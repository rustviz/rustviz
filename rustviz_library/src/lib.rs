use anyhow::Result;
// svg_generator
mod parse;
use rustviz_lib::svg_frontend::svg_generation;
use rustviz_lib::data::VisualizationData;

use std::collections::BTreeMap;

pub struct Rustviz{
  code_panel_svg : String,
  timeline_panel_svg : String,
}

impl Rustviz {
  pub fn new(a_src_str: &str, src_str: &str, main_str: &str) -> Result<Rustviz>{
    /* ******************************************
            --- Parse main.rs file ---
    ****************************************** */
    let (contents, line_num, var_map) = parse::parse_vars_to_map(main_str)?;
    let events = parse::extract_events(contents, line_num)?;
    /* ******************************************
            --- Build VisualizationData ---
    ****************************************** */
    let mut vd = VisualizationData {
      timelines: BTreeMap::new(),
      external_events: Vec::new(),
      preprocess_external_events: Vec::new(),
      event_line_map: BTreeMap::new()
    };
    parse::add_events(&mut vd, var_map, events)?;
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