use rustviz_lib::data::{ExternalEvent, LifetimeTrait, ResourceAccessPoint, Owner, MutRef, StaticRef, Function, Struct, VisualizationData, Visualizable};
use rustviz_lib::svg_frontend::svg_generation;
use std::collections::BTreeMap;
fn main() {
	
	let r = ResourceAccessPoint::Struct(Struct {
        hash: 1,
        owner: 1,
        name: String::from("r"),
        is_mut: false,
        lifetime_trait: LifetimeTrait::Copy,
        is_member: false,
	});
    let w = ResourceAccessPoint::Struct(Struct {
        hash: 2,
        owner: 1,
        name: String::from("r.w"),
        is_mut: false,
        lifetime_trait: LifetimeTrait::Copy,
        is_member: true,
    });
    let h = ResourceAccessPoint::Struct(Struct {
        hash: 3,
        owner: 1,
        name: String::from("r.h"),
        is_mut: false,
        lifetime_trait: LifetimeTrait::Copy,
        is_member: true,
    });
	let rect = ResourceAccessPoint::Struct(Struct {
        hash: 4,
        owner: 4,
        name: String::from("rect"),
        is_mut: false,
        lifetime_trait: LifetimeTrait::Copy,
        is_member: false,
	});
    let area = Some(ResourceAccessPoint::Function(Function {
        hash: 7,
        name: String::from("area(w: u32, h: u32)"),
    }));

    let f_func = Some(ResourceAccessPoint::Function(Function {
        hash: 5,
        name: String::from("struct Rect{}"),
    }));

    let print_func = Some(ResourceAccessPoint::Function(Function {
        hash: 8,
        name: String::from("println!()"),
    }));
    let mut vd = VisualizationData {
        timelines: BTreeMap::new(),
        external_events: Vec::new(),
        preprocess_external_events: Vec::new(),
        event_line_map: BTreeMap::new()
    };

    vd.append_external_event(ExternalEvent::Move{from: f_func.clone(),
        to: Some(r.clone())}, &(7 as usize));
    vd.append_external_event(ExternalEvent::Duplicate{from: None,
        to: Some(w.clone())}, &(8 as usize));
    vd.append_external_event(ExternalEvent::Duplicate{from: None,
        to: Some(h.clone())}, &(9 as usize));
    vd.append_external_event(ExternalEvent::PassByStaticReference{from: Some(r.clone()),
        to: area.clone()}, &(14 as usize));
    vd.append_external_event(ExternalEvent::StaticReturn{from: Some(r.clone()),
        to: print_func.clone()}, &(14 as usize));
    vd.append_external_event(ExternalEvent::InitializeParam{param: rect.clone()}, &(18 as usize));
    vd.append_external_event(ExternalEvent::GoOutOfScope{ ro : rect.clone() }, 
        &(20 as usize));
    vd.append_external_event(ExternalEvent::StructBox{from: Some(r.clone()), 
        to: Some(h.clone())}, &(16 as usize));
	
    vd.append_external_event(ExternalEvent::Move{ from : Some(r.clone()),
        to: None}, &(16 as usize));
    vd.append_external_event(ExternalEvent::GoOutOfScope{ ro : w.clone() }, 
        &(16 as usize));
    vd.append_external_event(ExternalEvent::GoOutOfScope{ ro : h.clone() }, 
        &(16 as usize));


    //rendering image
    svg_generation::render_svg(&"examples/struct_rect/input/".to_owned(), &"examples/struct_rect/".to_owned(), & mut vd);
}
