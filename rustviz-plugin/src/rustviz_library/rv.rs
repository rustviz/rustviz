use crate::svg_generator::{
  data::{ApproxNote, ExternalEvent, ShadowNote, VisualizationData},
  svg_frontend::svg_generation
};
use anyhow::Result;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// The rendered visualization. This is also the plugin's output format:
/// it is printed to stdout as one JSON object and read back by
/// `rustviz-lib`'s `PluginOutput`, so the two structs must agree.
#[derive(Serialize)]
pub struct Rustviz {
  /// SVG for the code panel (the annotated source listing).
  pub code_panel: String,
  /// SVG for the timeline panel.
  pub timeline_panel: String,
  /// Height in px shared by both panels.
  pub height: i32,
  /// RustViz 3 facts (`RV3_FACTS`, see `crate::facts`); absent otherwise.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub facts: Option<serde_json::Value>,
}

impl Rustviz {
  pub fn new(
    a_src_str: &str,
    src_str: &str,
    p_evts: Vec<(usize, ExternalEvent)>,
    ev_map: BTreeMap<usize, Vec<ExternalEvent>>,
    num_raps: usize,
    fn_source_lines: HashMap<u64, crate::facts::table::FunctionStart>,
    shadow_notes: HashMap<u64, ShadowNote>,
    binding_columns: Vec<(u64, u64)>,
    moves_out_first: Vec<usize>,
    temp_emptying_moves: Vec<usize>,
    hidden_source_lines: HashSet<usize>,
    approx_notes: Vec<ApproxNote>,
    state_table: crate::facts::table::StateTable,
    row_layout: crate::event_order::RowLayout,
  ) -> Result<Rustviz>{
    /* ******************************************
            --- Build VisualizationData ---
    ****************************************** */
    #[expect(clippy::disallowed_methods, reason = "each source column supplies its own initial header row")]
    let fn_start_lines = fn_source_lines.iter().map(|(&hash, start)| (hash, start.line)).collect();
    let source_lines: Vec<String> = src_str.lines().map(str::to_owned).collect();
    let mut vd = VisualizationData {
      timelines: BTreeMap::new(),
      external_events: Vec::new(),
      preprocess_external_events: p_evts,
      row_layout,
      event_line_map: ev_map,
      num_valid_raps: num_raps,
      fn_start_lines,
      fn_source_lines,
      shadow_notes,
      binding_columns,
      moves_out_first: moves_out_first.into_iter().collect(),
      temp_emptying_moves: temp_emptying_moves.into_iter().collect(),
      arrow_tracks: HashMap::new(),
      field_lenders: HashMap::new(),
      approx_notes,
      row_lines: Vec::new(),
      raised_headers: HashSet::new(),
      fork_above: HashSet::new(),
      exits_at_split: BTreeSet::new(),
      state_table,
      source_lines,
    };
    /* ******************************************
            --- Render SVG images ---
    ****************************************** */
    let (code_panel, timeline_panel, height) =
      svg_generation::render_svg(a_src_str, src_str, &mut vd, &hidden_source_lines);
    // A column of a variable of the same name as another in a separate
    // scope is named with marks the output doesn't show (#598).
    let code_panel = crate::expr_visitor_utils::display_text(&code_panel);
    let timeline_panel = crate::expr_visitor_utils::display_text(&timeline_panel);
    Ok(Rustviz { code_panel, timeline_panel, height, facts: None })
  }
}
