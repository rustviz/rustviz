use crate::svg_generator::{
  data::{ExternalEvent, ShadowNote, VisualizationData},
  svg_frontend::svg_generation
};
use anyhow::Result;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};

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
}

impl Rustviz {
  pub fn new(
    a_src_str: &str,
    src_str: &str,
    p_evts: Vec<(usize, ExternalEvent)>,
    ev_map: BTreeMap<usize, Vec<ExternalEvent>>,
    num_raps: usize,
    fn_start_lines: HashMap<u64, usize>,
    shadow_notes: HashMap<u64, ShadowNote>,
    binding_columns: Vec<(u64, u64)>,
    hidden_source_lines: HashSet<usize>,
  ) -> Result<Rustviz>{
    /* ******************************************
            --- Build VisualizationData ---
    ****************************************** */
    let mut vd = VisualizationData {
      timelines: BTreeMap::new(),
      external_events: Vec::new(),
      preprocess_external_events: p_evts,
      event_line_map: ev_map,
      num_valid_raps: num_raps,
      fn_start_lines,
      shadow_notes,
      binding_columns,
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
    Ok(Rustviz { code_panel, timeline_panel, height })
  }
}