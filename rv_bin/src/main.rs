use rustviz_library::Rustviz;
use anyhow::Result;
use std::env;
use std::path::Path;
use std::fs;
use std::fs::File;
use std::process::exit;

fn run_rustviz (path_to_dir: &Path) -> Result<()> {
  //read annotated src
  let annotated_src_path = path_to_dir.join("input/annotated_source.rs");
  if !annotated_src_path.exists() {
    eprintln!("Error: can't find annotated src!");
    exit(1);
  }

  let a_s = fs::read_to_string(annotated_src_path)?;
  
  let main_path = path_to_dir.join("main.rs");
  if !main_path.exists() {
    eprintln!("can't find main.rs!");
    exit(1);
  }

  let main_str = fs::read_to_string(main_path)?;
  
  let source_path = path_to_dir.join("source.rs");
  if !source_path.exists() {
    eprintln!("can't find source.rs!");
    exit(1);
  }

  let source_str = fs::read_to_string(source_path)?;
  
  let rv = Rustviz::new(&a_s, &source_str, &main_str)?;
  
  let code_panel_path = path_to_dir.join("vis_code.svg");
  let timeline_panel_path = path_to_dir.join("vis_timeline.svg");

  if !code_panel_path.exists(){
    File::create(code_panel_path.clone())?;
  }

  if !timeline_panel_path.exists(){
    File::create(timeline_panel_path.clone())?;
  }
  
  fs::write(code_panel_path, rv.code_panel())?;
  fs::write(timeline_panel_path, rv.timeline_panel())?;

  Ok(())
}

fn main(){
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
      eprintln!("Usage Error: cargo run <dirname>");
      exit(1);
    }

    let path_to_dir = Path::new("examples/").join(&args[1]);
    if !path_to_dir.is_dir() {
      eprintln!("Error: no corresponding directory exists in examples/!");
      exit(1);
      
    }

    match run_rustviz(&path_to_dir) {
      Ok(()) => (),
      Err(e) => {
        eprintln!("Error when generating visualization: {}", e);
        exit(1);
      }
    }
}
