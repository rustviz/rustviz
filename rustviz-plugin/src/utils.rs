use anyhow::{Result, anyhow};
use log::info;
use rustc_middle::ty::TyCtxt;
use crate::svg_generator::data::ExternalEvent;
use std::collections::{HashMap, BTreeMap};
use std::{path::PathBuf, fs};
use std::env::current_dir;
use crate::rustviz_library::rv::Rustviz;
use std::fs::File;

use crate::expr_visitor::RapData;

// toplevel annotation helpers
pub fn annotate_struct_field (
  line_str: &str,
  hash_map: & mut HashMap<String, usize>,
  a_map: & mut BTreeMap<usize, Vec<String>>,
  hashes: & mut usize,
  field: & rustc_hir::FieldDef,
  m: & TyCtxt
) {
  let name:String = field.ident.as_str().to_owned();
  let hash = *hash_map.entry(name.clone()).or_insert_with(|| {
    let current_hash = *hashes;
    *hashes = (*hashes + 1) % 10;
    current_hash
  });

  // Use the *ident* span, not the whole field span. `field.span`
  // covers the entire `x: i32` declaration; replacing that range
  // with just the field name eats the colon + type and the rendered
  // struct definition shows `x,` instead of `x: i32,`.
  let line: usize = m.sess.source_map().lookup_char_pos(field.ident.span.lo()).line;
  let left: usize = m.sess.source_map().lookup_char_pos(field.ident.span.lo()).col_display;
  let right: usize = m.sess.source_map().lookup_char_pos(field.ident.span.hi()).col_display;

  let mut line_contents = line_str.to_string();
  let replace_with = format!("[_tspan data-hash=\"{}\"_]{}[_/tspan_]", hash, name);
  line_contents.replace_range(left..right, &replace_with);
  let v = a_map.get_mut(&line).unwrap();
  if !v.contains(&line_contents) {
    v.push(line_contents);
  }
}


pub fn annotate_toplevel_fn (
  func_ident: rustc_span::symbol::Ident, 
  line_str: &str, 
  raps: & HashMap<String, RapData>,
  a_map: & mut BTreeMap<usize, Vec<String>>,
  hashes: & mut usize,
  m: &TyCtxt)  {
  let func_name = func_ident.as_str().to_owned();
  
  let line: usize = m.sess.source_map().lookup_char_pos(func_ident.span.lo()).line;
  let left: usize = m.sess.source_map().lookup_char_pos(func_ident.span.lo()).col_display;
  let right: usize = m.sess.source_map().lookup_char_pos(func_ident.span.hi()).col_display;
  let hash = match raps.get(&func_name) {
    Some(r) => { *r.rap.hash() }
    None => {
      let current_hash = *hashes;
      *hashes = (*hashes + 1) % 10;
      current_hash as u64
    }
  };


  let mut line_contents = line_str.to_string();
  let replace_with: String = format!("[_tspan class=\"fn\" data-hash=\"{}\" hash=\"{}\"_]{}[_/tspan_]", 0, hash, func_name);
  line_contents.replace_range(left..right, &replace_with);
  let v = a_map.get_mut(&line).unwrap();
  if !v.contains(&line_contents) {
    v.push(line_contents);
  }
}

pub fn annotate_enum_variant(
  ctor_name: &str, 
  parent_name: &str,
  variant: & rustc_hir::Variant,
  rap_map: & HashMap<String, RapData>,
  a_map: & mut BTreeMap<usize, Vec<String>>,
  m: & TyCtxt
) {
  let rap_name = format!("{}::{}", parent_name, ctor_name);
  if rap_map.contains_key(&rap_name) {
    let span = variant.ident.span;
    let hash = rap_map.get(&rap_name).unwrap().rap.hash();
    let line: usize = m.sess.source_map().lookup_char_pos(span.lo()).line;
    let left: usize = m.sess.source_map().lookup_char_pos(span.lo()).col_display;
    let line_str = &a_map[&line][0];
    let right = m.sess.source_map().lookup_char_pos(span.hi()).col_display;

    let mut line_contents = line_str.to_string();
    let replace_with = format!("[_tspan class=\"fn\" data-hash=\"{}\" hash=\"{}\"_]{}[_/tspan_]", 0, hash, ctor_name);
    line_contents.replace_range(left..right, &replace_with);
    let v = a_map.get_mut(&line).unwrap();
    if !v.contains(&line_contents) {
      v.push(line_contents);
    }
  }
}


pub struct RV1Helper {
  source_str: String,
  source_path: PathBuf
}

impl RV1Helper {
  pub fn new () -> RV1Helper {
    RV1Helper { source_str: String::new(), source_path: PathBuf::new() }
  }
  pub fn initialize_line_map(&mut self) -> Result<BTreeMap<usize, String>> {
    self.source_path = current_dir()?;
    self.source_path = self.source_path.join("src/lib.rs"); // could change this to whatever
    info!("source path {:#?}", self.source_path);
  
    let mut line_map: BTreeMap<usize, String> = BTreeMap::new();
  
  
    match fs::read_to_string(self.source_path.clone()) {
      Ok(contents) => {
        self.source_str = contents.clone(); // allows for comments in source string
        let mut res_str: String = String::new();
        // remove all comments from main string
        for line in contents.lines() {
          res_str.push_str(line);
          res_str.push('\n');
  
        }
        let lines: Vec<&str> = res_str.lines().collect();
        for (line_num, line_content) in lines.iter().enumerate() {
          line_map.insert(line_num + 1, line_content.to_string());
        }
      }
  
      Err(e) => {
        return Err(anyhow!("Error with reading source file : {}", e));
      }
    }

    //println!("BT MAP: {:#?}", line_map);
    return Ok(line_map);
  }

  pub fn generate_vis(& mut self,
    mut line_map: BTreeMap<usize, Vec<ExternalEvent>>,
    p_events: Vec<(usize, ExternalEvent)>,
    a_map: & mut BTreeMap<usize, Vec<String>>,
    num_raps: usize,
    fn_start_lines: HashMap<u64, usize>,
    write_to_cwd: bool) -> Result<()> {
    let mut keys_to_remove: Vec<usize> = Vec::new();
    for (k, v) in line_map.iter() {
      if v.is_empty() {
        keys_to_remove.push(*k);
      }
    }

    for k in keys_to_remove.iter() {
      line_map.remove(k);
    }

    let annotated_source_str: String = generate_annotated_src(a_map);
    //println!("ANNOTATED : \n{}", annotated_source_str);


    // send stuff to RV1
    let rv = Rustviz::new(&annotated_source_str, &self.source_str, p_events, line_map, num_raps, fn_start_lines)?;

    if write_to_cwd { // write the SVG files
      self.source_path.pop(); // just write to inside cwd
      let code_panel_path: PathBuf = self.source_path.join("vis_code.svg");
      let timeline_panel_path: PathBuf = self.source_path.join("vis_timeline.svg");
  
      if !code_panel_path.exists(){
        File::create(code_panel_path.clone())?;
      }
    
      if !timeline_panel_path.exists(){
        File::create(timeline_panel_path.clone())?;
      }
  
      fs::write(code_panel_path, rv.code_panel())?;
      fs::write(timeline_panel_path, rv.timeline_panel())?;
    }
    else {
      // write SVG files to stdio
      let res = format!("{}:::{}", rv.code_panel(), rv.timeline_panel());
      println!("{res}");
    }
    Ok(())
  }
  

}

/// Merge a set of annotations of the same source line into a single line
/// that contains every annotation's `[_..._]…[_/_]` wrappers.
///
/// Each input string is the underlying source line with zero or more
/// `[_open_]name[_/close_]` regions inserted (see `annotate_src`). Stripping
/// every `[_..._]` from each input must yield the same underlying text, which
/// is the invariant the merge relies on.
///
/// The original implementation walked `strings[0]` character-by-character and
/// assumed that, between any two consecutive characters of the underlying
/// text, every other string had **at most one** `[_..._]` marker. That broke
/// (with `assert_eq!(char_at_i, '[')`) any time two annotations landed at
/// the same source position — increasingly common after rustc 1.91 (HIR
/// span behavior changed). This rewrite tolerates arbitrary numbers of
/// adjacent markers from multiple strings and is robust to inputs whose
/// underlying text mismatches: it falls back to emitting whatever the first
/// non-exhausted string has rather than panicking.
fn union_strings(strings: &Vec<String>) -> String {
  if strings.len() == 1 {
    return strings[0].clone();
  }
  if strings.len() == 2 {
    // Convention: index 0 is the bare source line, index 1 is the only
    // annotated copy — short-circuit to the annotated one.
    return strings[1].clone();
  }

  let bufs: Vec<Vec<char>> = strings.iter().map(|s| s.chars().collect()).collect();
  let mut offsets = vec![0usize; bufs.len()];
  let mut res = String::new();

  loop {
    // Emit every leading `[_..._]` marker from any input until none remain.
    // We loop here because two strings can each carry a marker at the same
    // underlying position, and a single string can carry several markers
    // back-to-back (e.g. `[_open_][_close_]`).
    let mut emitted_marker = true;
    while emitted_marker {
      emitted_marker = false;
      for k in 0..bufs.len() {
        if offsets[k] < bufs[k].len() && bufs[k][offsets[k]] == '[' {
          // Copy from `[` through the matching `]` into res, advance offset.
          while offsets[k] < bufs[k].len() {
            let c = bufs[k][offsets[k]];
            res.push(c);
            offsets[k] += 1;
            if c == ']' { break; }
          }
          emitted_marker = true;
        }
      }
    }

    // Pick the next underlying char from the first non-exhausted input;
    // advance every input whose next char matches.
    let chosen = bufs.iter().zip(offsets.iter()).find_map(|(buf, &o)| {
      if o < buf.len() { Some(buf[o]) } else { None }
    });
    let Some(ch) = chosen else { break };

    res.push(ch);
    for k in 0..bufs.len() {
      if offsets[k] < bufs[k].len() && bufs[k][offsets[k]] == ch {
        offsets[k] += 1;
      }
    }
  }
  res
}

pub fn generate_annotated_src(annotated_line_map: & mut BTreeMap<usize, Vec<String>>) -> String {
  let mut annotated_str = String::new();
  for (_k, v) in annotated_line_map {
    annotated_str.push_str(&union_strings(v));
    annotated_str.push('\n');
  }
  annotated_str = annotated_str.replace("&", "&amp;");
  annotated_str = annotated_str.replace("<", "&lt;");
  annotated_str = annotated_str.replace(">", "&gt;");
  annotated_str = annotated_str.replace("[_", "<");
  annotated_str = annotated_str.replace("_]", ">");

  annotated_str
}