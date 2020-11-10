use rustviz_lib::data::{ExternalEvent, LifetimeTrait, ResourceAccessPoint, Owner, MutRef, StaticRef, Function, VisualizationData, Visualizable};
use rustviz_lib::svg_frontend::svg_generation;
use std::collections::BTreeMap;
fn main() {
    
    let x = ResourceAccessPoint::Owner(Owner {
        hash: 1,
        name: String::from("x"),
        is_mut: true,
        lifetime_trait: LifetimeTrait::Move,
    });
    let y = ResourceAccessPoint::MutRef(MutRef {
        hash: 2,
        my_owner_hash: Some(1),
        name: String::from("y"),
        is_mut: false,
        lifetime_trait: LifetimeTrait::Move,
    });
    let z = ResourceAccessPoint::MutRef(MutRef {
        hash: 3,
        my_owner_hash: Some(1),
        name: String::from("z"),
        is_mut: false,
        lifetime_trait: LifetimeTrait::Move,
    });
    let s = ResourceAccessPoint::MutRef(MutRef {
        hash: 4,
        my_owner_hash: Some(1),
        name: String::from("z"),
        is_mut: false,
        lifetime_trait: LifetimeTrait::Move,
    });
    let string_ctor = Some(ResourceAccessPoint::Function(Function {
        hash: 5,
        name: String::from("String::from()"),
    }));

    let world_func = Some(ResourceAccessPoint::Function(Function {
        hash: 6,
        name: String::from("world()"),
    }));
    let push_func = Some(ResourceAccessPoint::Function(Function {
        hash: 7,
        name: String::from("push_str()"),
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

    vd.append_external_event(ExternalEvent::Move{from: string_ctor.clone(),
        to: Some(x.clone())}, &(2 as usize));
    vd.append_external_event(ExternalEvent::MutableBorrow{from: Some(x.clone()),
        to: Some(y.clone())}, &(3 as usize));
    vd.append_external_event(ExternalEvent::PassByMutableReference{from: Some(y.clone()),
        to: world_func.clone()}, &(4 as usize));
    vd.append_external_event(ExternalEvent::MutableReturn{from: Some(y.clone()), 
        to: Some(x.clone())}, &(4 as usize));
    vd.append_external_event(ExternalEvent::MutableBorrow{from: Some(x.clone()),
        to: Some(z.clone())}, &(5 as usize));
    vd.append_external_event(ExternalEvent::PassByMutableReference{from: Some(z.clone()),
        to: world_func.clone()}, &(6 as usize));
    vd.append_external_event(ExternalEvent::MutableReturn{from: Some(z.clone()), 
        to: Some(x.clone())}, &(6 as usize));
    vd.append_external_event(ExternalEvent::PassByMutableReference{from: Some(x.clone()),
        to: push_func.clone()}, &(7 as usize));
    vd.append_external_event(ExternalEvent::PassByStaticReference{from: Some(x.clone()),
        to: print_func.clone()}, &(8 as usize));
    vd.append_external_event(ExternalEvent::GoOutOfScope{ ro : x.clone() }, 
        &(9 as usize));
    vd.append_external_event(ExternalEvent::GoOutOfScope{ ro : y.clone() }, 
        &(9 as usize));
    vd.append_external_event(ExternalEvent::GoOutOfScope{ ro : z.clone() }, 
        &(9 as usize));

    vd.append_external_event(ExternalEvent::InitializeParam{param: s.clone()}, &(11 as usize));
    vd.append_external_event(ExternalEvent::PassByMutableReference{from: Some(s.clone()),
        to: push_func.clone()}, &(12 as usize));
    vd.append_external_event(ExternalEvent::GoOutOfScope{ ro : s.clone() }, 
        &(13 as usize));
    

    //rendering image
    svg_generation::render_svg(&"examples/nll_lexical_scope_different/input/".to_owned(), &"examples/nll_lexical_scope_different/".to_owned(), & mut vd);
}
