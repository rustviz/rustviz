use rustviz_lib::data::{ExternalEvent, LifetimeTrait, ResourceAccessPoint, Owner, MutRef, StaticRef, Function, VisualizationData, Visualizable};
use rustviz_lib::svg_frontend::svg_generation;
use std::collections::BTreeMap;
fn main() {
    
    let x = ResourceAccessPoint::Owner(Owner {
        hash: 1,
        name: String::from("x"),
        is_mut: false,
        lifetime_trait: LifetimeTrait::Move,
    });
    let y = ResourceAccessPoint::StaticRef(StaticRef {
        hash: 2,
        name: String::from("y"),
        my_owner_hash: Some(1),
        is_mut: false,
        lifetime_trait: LifetimeTrait::Copy,
    });
    let z = ResourceAccessPoint::StaticRef(StaticRef {
        hash: 3,
        name: String::from("z"),
        my_owner_hash: Some(1),
        is_mut: false,
        lifetime_trait: LifetimeTrait::Copy,
    });
    let s1 = ResourceAccessPoint::StaticRef(StaticRef {
        hash: 4,
        my_owner_hash: Some(1),
        name: String::from("s1"),
        is_mut: false,
        lifetime_trait: LifetimeTrait::Copy,
    });
    let s2 = ResourceAccessPoint::StaticRef(StaticRef {
        hash: 5,
        my_owner_hash: Some(1),
        name: String::from("s2"),
        is_mut: false,
        lifetime_trait: LifetimeTrait::Copy,
    });
    let string_ctor = Some(ResourceAccessPoint::Function(Function {
        hash: 6,
        name: String::from("String::from()"),
    }));

    let f_func = Some(ResourceAccessPoint::Function(Function {
        hash: 7,
        name: String::from("f()"),
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
    vd.append_external_event(ExternalEvent::StaticBorrow{from: Some(x.clone()),
        to: Some(y.clone())}, &(3 as usize));
    vd.append_external_event(ExternalEvent::StaticBorrow{from: Some(x.clone()),
        to: Some(z.clone())}, &(4 as usize));
    vd.append_external_event(ExternalEvent::Duplicate{from: Some(y.clone()),
        to: f_func.clone()}, &(5 as usize));
    vd.append_external_event(ExternalEvent::Duplicate{from: Some(z.clone()),
        to: f_func.clone()}, &(5 as usize));
    vd.append_external_event(ExternalEvent::GoOutOfScope{ ro : x.clone() }, 
        &(6 as usize));
    vd.append_external_event(ExternalEvent::GoOutOfScope{ ro : y.clone() }, 
        &(6 as usize));
    vd.append_external_event(ExternalEvent::GoOutOfScope{ ro : z.clone() }, 
        &(6 as usize));

    vd.append_external_event(ExternalEvent::InitializeParam{param: s1.clone()}, &(8 as usize));
    vd.append_external_event(ExternalEvent::InitializeParam{param: s2.clone()}, &(8 as usize));
    vd.append_external_event(ExternalEvent::PassByStaticReference{from: Some(s1.clone()),
        to: print_func.clone()}, &(9 as usize));
    vd.append_external_event(ExternalEvent::PassByStaticReference{from: Some(s2.clone()),
        to: print_func.clone()}, &(9 as usize));
    vd.append_external_event(ExternalEvent::GoOutOfScope{ ro : s1.clone() }, 
        &(10 as usize));
    vd.append_external_event(ExternalEvent::GoOutOfScope{ ro : s2.clone() }, 
        &(10 as usize));

    //rendering image
    svg_generation::render_svg(&"examples/multiple_immutable_borrow/input/".to_owned(), &"examples/multiple_immutable_borrow/".to_owned(), & mut vd);
}
