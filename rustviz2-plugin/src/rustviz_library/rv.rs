use crate::svg_generator::{
  data::{ExternalEvent, VisualizationData},
  svg_frontend::svg_generation
};
use anyhow::Result;
use std::collections::{BTreeMap, HashMap};

pub struct Rustviz{
  code_panel_svg : String,
  timeline_panel_svg : String,
}

impl Rustviz {
  pub fn new(
    a_src_str: &str,
    src_str: &str,
    p_evts: Vec<(usize, ExternalEvent)>,
    ev_map: BTreeMap<usize, Vec<ExternalEvent>>,
    num_raps: usize,
    fn_start_lines: HashMap<u64, usize>,
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
    };
    /* ******************************************
            --- Render SVG images ---
    ****************************************** */
    let res = svg_generation::render_svg(a_src_str, src_str, &mut vd);
    Ok(Rustviz {
      code_panel_svg : res.0,
      timeline_panel_svg: res.1
    })
  }

  pub fn code_panel(&self) -> String {
    self.code_panel_svg.clone()
  }
  
  pub fn timeline_panel(&self) -> String {
    self.timeline_panel_svg.clone()
  }
}