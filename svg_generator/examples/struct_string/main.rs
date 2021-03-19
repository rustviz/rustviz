use rustviz_lib::data::{ExternalEvent, LifetimeTrait, ResourceAccessPoint, Owner, MutRef, StaticRef, Function, Struct, VisualizationData, Visualizable};
use rustviz_lib::svg_frontend::svg_generation;
use std::collections::BTreeMap;
fn main() {
	
	let f = ResourceAccessPoint::Struct(Struct {
        hash: 1,
        owner: 1,
        name: String::from("f"),
        is_mut: false,
        lifetime_trait: LifetimeTrait::Move,
        is_member: false,
	});
    let x = ResourceAccessPoint::Struct(Struct {
        hash: 2,
        owner: 1,
        name: String::from("f.x"),
        is_mut: false,
        lifetime_trait: LifetimeTrait::Move,
        is_member: true,
    });
    let y = ResourceAccessPoint::Struct(Struct {
        hash: 3,
        owner: 1,
        name: String::from("f.y"),
        is_mut: false,
        lifetime_trait: LifetimeTrait::Move,
        is_member: true,
    });
	let _y = ResourceAccessPoint::Owner(Owner {
        hash: 4,
        name: String::from("_y"),
        is_mut: false,
        lifetime_trait: LifetimeTrait::Move,
    });
    let string_ctor = Some(ResourceAccessPoint::Function(Function {
        hash: 5,
        name: String::from("String::from()"),
    }));

    let f_func = Some(ResourceAccessPoint::Function(Function {
        hash: 6,
        name: String::from("struct Foo{}"),
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

    vd.append_external_event(ExternalEvent::StructBox{from: Some(f.clone()), 
        to: Some(y.clone())}, &(11 as usize));
    vd.append_external_event(ExternalEvent::Move{from: string_ctor.clone(),
		to: Some(_y.clone())}, &(7 as usize));
    vd.append_external_event(ExternalEvent::Move{from: Some(_y.clone()),
        to: Some(y.clone())}, &(8 as usize));
    vd.append_external_event(ExternalEvent::GoOutOfScope{ ro : _y.clone() }, 
        &(9 as usize));

	vd.append_external_event(ExternalEvent::Move{from: f_func.clone(),
		to: Some(f.clone())}, &(8 as usize));
    vd.append_external_event(ExternalEvent::InitializeParam{param: f.clone()}, &(8 as usize));
	vd.append_external_event(ExternalEvent::InitializeParam{param: x.clone()}, &(8 as usize));
    vd.append_external_event(ExternalEvent::PassByStaticReference{from: Some(x.clone()),
        to: print_func.clone()}, &(9 as usize));
    vd.append_external_event(ExternalEvent::PassByStaticReference{from: Some(y.clone()),
        to: print_func.clone()}, &(10 as usize));
	
    vd.append_external_event(ExternalEvent::GoOutOfScope{ ro : f.clone() }, 
        &(11 as usize));
    vd.append_external_event(ExternalEvent::GoOutOfScope{ ro : x.clone() }, 
        &(11 as usize));
    vd.append_external_event(ExternalEvent::GoOutOfScope{ ro : y.clone() }, 
        &(11 as usize));
    

    //rendering image
    svg_generation::render_svg(&"examples/struct_string/input/".to_owned(), &"examples/struct_string/".to_owned(), & mut vd);
}
