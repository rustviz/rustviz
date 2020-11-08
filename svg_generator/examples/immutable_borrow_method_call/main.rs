use rustviz_lib::data::{ExternalEvent, LifetimeTrait, ResourceAccessPoint, Owner, MutRef, StaticRef, Function, VisualizationData, Visualizable};
use rustviz_lib::svg_frontend::svg_generation;
use std::collections::BTreeMap;
fn main() {
    let s = ResourceAccessPoint::Owner(Owner {
        hash: 1,
        name: String::from("s"),
        is_mut: false,
        lifetime_trait: LifetimeTrait::Move,
    });
    let len1 = ResourceAccessPoint::Owner(Owner {
        hash: 2,
        name: String::from("len1"),
        is_mut: false,
        lifetime_trait: LifetimeTrait::Copy,
    });
    let len2 = ResourceAccessPoint::Owner(Owner {
        hash: 3,
        name: String::from("len2"),
        is_mut: false,
        lifetime_trait: LifetimeTrait::Copy,
    });
    let string_ctor = Some(ResourceAccessPoint::Function(Function {
        hash: 5,
        name: String::from("String::from()"),
    }));
    let len1_func = Some(ResourceAccessPoint::Function(Function {
        hash: 6,
        name: String::from("String::len()"),
    }));
    let len2_func = Some(ResourceAccessPoint::Function(Function {
        hash: 7,
        name: String::from("len()"),
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
        to: Some(s.clone())}, &(2 as usize));
    vd.append_external_event(ExternalEvent::PassByStaticReference{from: Some(s.clone()),
        to: len1_func.clone()}, &(3 as usize));
    vd.append_external_event(ExternalEvent::Duplicate{from: len1_func.clone(),
        to: Some(len1.clone())}, &(3 as usize));
    vd.append_external_event(ExternalEvent::PassByStaticReference{from: Some(s.clone()),
        to: len2_func.clone()}, &(4 as usize));
    vd.append_external_event(ExternalEvent::Duplicate{from: len2_func.clone(),
        to: Some(len2.clone())}, &(4 as usize));
    vd.append_external_event(ExternalEvent::PassByStaticReference{from: Some(len1.clone()),
        to: print_func.clone()}, &(5 as usize));
    vd.append_external_event(ExternalEvent::PassByStaticReference{from: Some(len2.clone()),
        to: print_func.clone()}, &(5 as usize));
    vd.append_external_event(ExternalEvent::GoOutOfScope{ ro : s.clone() }, 
        &(6 as usize));
    vd.append_external_event(ExternalEvent::GoOutOfScope{ ro : len1.clone() }, 
        &(6 as usize));
    vd.append_external_event(ExternalEvent::GoOutOfScope{ ro : len2.clone() }, 
        &(6 as usize));

    //rendering image
    svg_generation::render_svg(&"examples/immutable_borrow_method_call/input/".to_owned(), &"examples/immutable_borrow_method_call/".to_owned(), &mut vd);
}