//! Export command: write graph.json, graph.html, and GRAPH_REPORT.md.
//!
//! These three outputs are the human-facing face of GraphSwarm.
//! Commit them to git so the whole team benefits without re-indexing.
//!
//!   graph.json       -full serialized CallGraph (machine-readable)
//!   graph.html       -interactive browser visualization (human-readable)
//!   GRAPH_REPORT.md  -key concepts, god nodes, surprising connections

use crate::error::Result;
use crate::indexer::call_graph::{CallGraph, GraphMetadata};
use crate::storage::{GraphStore, KvBackend};
use clap::Args;
use std::path::{Path, PathBuf};

#[derive(Args)]
pub struct ExportCommand {
    /// Output directory (default: graphswarm-out/)
    #[arg(short, long, default_value = "graphswarm-out")]
    pub output: String,

    /// Export format: json, html, markdown, all
    #[arg(short, long, default_value = "all")]
    pub format: String,

    /// Path to repository root (where .graphswarm_db lives)
    #[arg(default_value = ".")]
    pub path: String,
}

impl ExportCommand {
    pub async fn execute(&self) -> Result<()> {
        let repo_root = PathBuf::from(&self.path);
        let db_path = repo_root.join(".graphswarm").join("db");
        let out_dir = PathBuf::from(&self.output);

        std::fs::create_dir_all(&out_dir)
            .map_err(|e| crate::error::Error::storage(format!("Cannot create output dir: {e}")))?;

        let kv = KvBackend::open(&db_path)?;
        let store = GraphStore::new(kv);
        let mut graph = store.load_graph()?;
        let meta = graph.metadata.clone();

        // Filter out dangling edges: edges where the callee was never indexed
        // (e.g. calls to stdlib functions like thread::sleep, println!, vec!).
        // D3.js crashes with "node not found" if a link references an ID that
        // has no corresponding node in the nodes array.
        {
            use std::collections::HashSet;
            let entity_ids: HashSet<&String> = graph.entities.keys().collect();
            let before = graph.edges.len();
            graph
                .edges
                .retain(|(src, tgt)| entity_ids.contains(src) && entity_ids.contains(tgt));
            let filtered = before - graph.edges.len();
            if filtered > 0 {
                println!("Filtered {filtered} dangling edges (external/stdlib calls)");
            }
        }

        match self.format.as_str() {
            "json" => self.export_json(&graph, &out_dir)?,
            "html" => self.export_html(&graph, &out_dir)?,
            "markdown" => self.export_markdown(&graph, &meta, &out_dir)?,
            _ => {
                // "all" or anything unrecognised → write everything
                self.export_json(&graph, &out_dir)?;
                self.export_html(&graph, &out_dir)?;
                self.export_markdown(&graph, &meta, &out_dir)?;
            }
        }

        println!("Exported to {}/", out_dir.display());
        Ok(())
    }

    /// Writes `graph.json` -full serialized CallGraph.
    fn export_json(&self, graph: &CallGraph, out_dir: &Path) -> Result<()> {
        let path = out_dir.join("graph.json");
        let json = serde_json::to_string_pretty(graph)
            .map_err(|e| crate::error::Error::serialization(format!("JSON export failed: {e}")))?;
        std::fs::write(&path, json)
            .map_err(|e| crate::error::Error::storage(format!("Cannot write graph.json: {e}")))?;
        println!(
            "  graph.json ({} entities, {} edges)",
            graph.entities.len(),
            graph.edges.len()
        );
        Ok(())
    }

    /// Writes `graph.html` -interactive D3.js force-directed visualization.
    ///
    /// D3.js v7.9.0 is bundled inline (src/cli/assets/d3.min.js) so the
    /// visualization works with no internet connection.
    ///
    /// Why four-part concatenation instead of one big format!() string?
    /// D3.js itself contains many `{` and `}` characters, which would need
    /// escaping as `{{`/`}}` inside format!(). Concatenating it as a plain
    /// &str avoids that entirely.
    fn export_html(&self, graph: &CallGraph, out_dir: &Path) -> Result<()> {
        let path = out_dir.join("graph.html");

        let nodes: Vec<serde_json::Value> = graph
            .entities
            .values()
            .map(|e| {
                serde_json::json!({
                    "id":   e.id,
                    "name": e.name,
                    "file": e.file_path,
                    "type": format!("{}", e.entity_type),
                    "lang": format!("{}", e.language),
                })
            })
            .collect();

        let links: Vec<serde_json::Value> = graph
            .edges
            .iter()
            .map(|(src, tgt)| serde_json::json!({ "source": src, "target": tgt }))
            .collect();

        let nodes_json = serde_json::to_string(&nodes).unwrap_or_default();
        let links_json = serde_json::to_string(&links).unwrap_or_default();
        let repo_path = &graph.metadata.repo_path;
        let indexed_at = &graph.metadata.indexed_at;
        let n_entities = graph.entities.len();
        let n_edges = graph.edges.len();

        // ── Part 1: static HTML structure (uses repo metadata format args) ────
        // r##"..."## avoids early termination from CSS hex colors like "#d2a8ff".
        let part1 = format!(
            r##"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<title>GraphSwarm &mdash; {repo_path}</title>
<style>
  * {{ box-sizing: border-box; margin: 0; padding: 0; }}
  body {{ background: #0d1117; color: #e6edf3; font-family: monospace; }}
  #header {{ padding: 12px 20px; background: #161b22; border-bottom: 1px solid #30363d; }}
  #header h1 {{ font-size: 16px; color: #58a6ff; }}
  #header p  {{ font-size: 12px; color: #8b949e; margin-top: 4px; }}
  #canvas {{ width: 100%; height: calc(100vh - 60px); }}
  .tooltip {{
    position: absolute; background: #161b22; border: 1px solid #30363d;
    padding: 8px 12px; border-radius: 6px; font-size: 12px;
    pointer-events: none; opacity: 0; transition: opacity 0.15s;
  }}
  #search {{
    position: absolute; top: 72px; right: 16px;
    background: #161b22; border: 1px solid #30363d; border-radius: 6px;
    padding: 6px 10px; color: #e6edf3; font-family: monospace; font-size: 13px;
    outline: none; width: 220px;
  }}
  .node circle {{ stroke-width: 1.5px; cursor: pointer; }}
  .node text {{ font-size: 11px; fill: #8b949e; pointer-events: none; }}
  .link {{ stroke: #30363d; stroke-opacity: 0.6; }}
  .node.highlighted circle {{ stroke: #f78166; stroke-width: 2.5px; }}
  .link.highlighted {{ stroke: #58a6ff; stroke-opacity: 1.0; }}
</style>
</head>
<body>
<div id="header">
  <h1>GraphSwarm &mdash; {repo_path}</h1>
  <p>Indexed: {indexed_at} &middot; {n_entities} entities &middot; {n_edges} edges</p>
</div>
<input id="search" placeholder="Search entities..." />
<div class="tooltip" id="tooltip"></div>
<svg id="canvas"></svg>
"##
        );

        // ── Part 2: D3.js v7.9.0 inline (no format! -D3 contains { and }) ───
        // Concatenated as plain string; no macro processing touches D3 source.
        let part2 = String::from("<script>\n/* D3.js v7.9.0 -bundled for offline use */\n")
            + super::assets::D3_MIN_JS
            + "\n</script>\n";

        // ── Part 3: dynamic data (uses {nodes_json}, {links_json}) ────────────
        let part3 = format!(
            r##"<script>
const NODES = {nodes_json};
const LINKS = {links_json};
"##
        );

        // ── Part 4: static app logic (no format args) ─────────────────────────
        let part4 = r##"const COLOR = {
  "function": "#58a6ff", "method": "#3fb950", "class": "#d2a8ff",
  "import":   "#ffa657", "module":  "#ff7b72"
};
const svg     = d3.select("#canvas");
const tooltip = document.getElementById("tooltip");
const g = svg.append("g");
svg.call(d3.zoom().scaleExtent([0.1, 4]).on("zoom", e => g.attr("transform", e.transform)));
const sim = d3.forceSimulation(NODES)
  .force("link",    d3.forceLink(LINKS).id(d => d.id).distance(80))
  .force("charge",  d3.forceManyBody().strength(-120))
  .force("center",  d3.forceCenter(window.innerWidth / 2, (window.innerHeight - 60) / 2))
  .force("collide", d3.forceCollide(18));
const link = g.append("g").selectAll(".link")
  .data(LINKS).enter().append("line").attr("class", "link");
const node = g.append("g").selectAll(".node")
  .data(NODES).enter().append("g").attr("class", "node")
  .call(d3.drag()
    .on("start", (e, d) => { if (!e.active) sim.alphaTarget(0.3).restart(); d.fx = d.x; d.fy = d.y; })
    .on("drag",  (e, d) => { d.fx = e.x; d.fy = e.y; })
    .on("end",   (e, d) => { if (!e.active) sim.alphaTarget(0); d.fx = null; d.fy = null; }));
node.append("circle").attr("r", 7)
  .attr("fill", d => COLOR[d.type] || "#8b949e").attr("stroke", "#0d1117");
node.append("text").text(d => d.name).attr("dx", 10).attr("dy", 4);
node.on("mouseover", (e, d) => {
    tooltip.style.opacity = 1;
    tooltip.style.left = (e.pageX + 12) + "px";
    tooltip.style.top  = (e.pageY - 20) + "px";
    tooltip.innerHTML  = "<b>" + d.name + "</b><br>" + d.type + "<br><small>" + d.file + "</small>";
  })
  .on("mousemove", e => { tooltip.style.left = (e.pageX + 12) + "px"; tooltip.style.top = (e.pageY - 20) + "px"; })
  .on("mouseout",  () => { tooltip.style.opacity = 0; });
node.on("click", (_, d) => {
    const ids = new Set([d.id]);
    LINKS.forEach(l => {
        if (l.source.id === d.id || l.target.id === d.id) { ids.add(l.source.id); ids.add(l.target.id); }
    });
    node.classed("highlighted", n => ids.has(n.id));
    link.classed("highlighted", l => l.source.id === d.id || l.target.id === d.id);
});
document.getElementById("search").addEventListener("input", function() {
    const q = this.value.toLowerCase();
    node.select("circle").attr("r", d => (q && d.name.toLowerCase().includes(q)) ? 12 : 7);
});
sim.on("tick", () => {
    link.attr("x1", d => d.source.x).attr("y1", d => d.source.y)
        .attr("x2", d => d.target.x).attr("y2", d => d.target.y);
    node.attr("transform", d => "translate(" + d.x + "," + d.y + ")");
});
</script>
</body>
</html>"##;

        let html = part1 + &part2 + &part3 + part4;

        std::fs::write(&path, html)
            .map_err(|e| crate::error::Error::storage(format!("Cannot write graph.html: {e}")))?;
        println!("  graph.html (interactive, D3.js bundled -works offline)");
        Ok(())
    }

    /// Writes `GRAPH_REPORT.md` -human-readable analysis of the call graph.
    ///
    /// Includes god nodes, largest files, cross-module edges, and suggested questions.
    fn export_markdown(
        &self,
        graph: &CallGraph,
        meta: &GraphMetadata,
        out_dir: &Path,
    ) -> Result<()> {
        use std::collections::HashMap;

        let path = out_dir.join("GRAPH_REPORT.md");

        // God nodes: entities with highest total degree (in + out)
        let mut degree: HashMap<&str, usize> = HashMap::new();
        for (caller, callee) in &graph.edges {
            *degree.entry(caller.as_str()).or_default() += 1;
            *degree.entry(callee.as_str()).or_default() += 1;
        }
        let mut sorted: Vec<(&&str, &usize)> = degree.iter().collect();
        sorted.sort_by(|a, b| b.1.cmp(a.1));
        let god_nodes: Vec<String> = sorted
            .iter()
            .take(5)
            .map(|(id, deg)| {
                if let Some(e) = graph.entities.get(**id) {
                    format!("- **{}** (`{}`) -{} connections", e.name, e.file_path, deg)
                } else {
                    format!("- `{}` -{} connections", id, deg)
                }
            })
            .collect();

        // Cross-module edges: caller and callee in different top-level directories
        let cross_edges: Vec<String> = graph
            .edges
            .iter()
            .filter_map(|(caller, callee)| {
                let cd = caller.split('/').nth(1).unwrap_or("");
                let td = callee.split('/').nth(1).unwrap_or("");
                if cd != td && !cd.is_empty() && !td.is_empty() {
                    let cn = caller.split("::").last().unwrap_or(caller);
                    let tn = callee.split("::").last().unwrap_or(callee);
                    Some(format!("- `{cn}` → `{tn}` (cross-module)"))
                } else {
                    None
                }
            })
            .take(5)
            .collect();

        // Largest files by entity count
        let mut file_counts: HashMap<&String, usize> = HashMap::new();
        for e in graph.entities.values() {
            *file_counts.entry(&e.file_path).or_default() += 1;
        }
        let mut by_file: Vec<_> = file_counts.into_iter().collect();
        by_file.sort_by_key(|&(_, n)| std::cmp::Reverse(n));
        let largest: Vec<String> = by_file
            .iter()
            .take(5)
            .map(|(f, n)| format!("- `{f}` -{n} entities"))
            .collect();

        let report = format!(
            r#"# GraphSwarm Report

**Repository:** {repo}
**Indexed:** {indexed}
**Entities:** {entities} across {files} files
**Call edges:** {edges}

---

## God Nodes

The most connected entities -everything flows through these.

{god}

---

## Largest Files

Files with the most entities -high complexity areas.

{largest}

---

## Surprising Connections

Cross-module call edges -unexpected dependencies between directories.

{cross}

---

## Suggested Questions

```bash
graphswarm query "authentication flow"
graphswarm query callers src/auth.rs::verify_token
graphswarm query bfs src/main.rs::main 3
graphswarm server   # start MCP server for Claude Code
```

*Generated by GraphSwarm v{ver}*
"#,
            repo = meta.repo_path,
            indexed = meta.indexed_at,
            entities = meta.total_entities,
            files = meta.total_files,
            edges = graph.edges.len(),
            god = if god_nodes.is_empty() {
                "_(none found)_".into()
            } else {
                god_nodes.join("\n")
            },
            largest = if largest.is_empty() {
                "_(none found)_".into()
            } else {
                largest.join("\n")
            },
            cross = if cross_edges.is_empty() {
                "_(none -clean module boundaries!)_".into()
            } else {
                cross_edges.join("\n")
            },
            ver = env!("CARGO_PKG_VERSION"),
        );

        std::fs::write(&path, report).map_err(|e| {
            crate::error::Error::storage(format!("Cannot write GRAPH_REPORT.md: {e}"))
        })?;
        println!("  GRAPH_REPORT.md (god nodes, connections, suggested questions)");
        Ok(())
    }
}
