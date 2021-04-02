use rustviz_lib::data::{ExternalEvent, LifetimeTrait, ResourceAccessPoint, Owner, MutRef, StaticRef, Function, VisualizationData, Visualizable};
use rustviz_lib::svg_frontend::svg_generation;
use std::collections::BTreeMap;
fn main() {
    let s1 = ResourceAccessPoint::Owner(Owner {
        hash: 1,
        name: String::from("s1"),
        is_mut: true,
        lifetime_trait: LifetimeTrait::Move,
    });
    let s2 = ResourceAccessPoint::Owner(Owner {
        hash: 2,
        name: String::from("s2"),
        is_mut: false,
        lifetime_trait: LifetimeTrait::Move,
    });
    let string_ctor = Some(ResourceAccessPoint::Function(Function {
        hash: 5,
        name: String::from("String::from()"),
    }));
    let push1_func = Some(ResourceAccessPoint::Function(Function {
        hash: 6,
        name: String::from("String::push_str()"),
    }));
    let push2_func = Some(ResourceAccessPoint::Function(Function {
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
        to: Some(s1.clone())}, &(2 as usize));
    vd.append_external_event(ExternalEvent::Move{from: string_ctor.clone(),
        to: Some(s2.clone())}, &(3 as usize));
    vd.append_external_event(ExternalEvent::PassByMutableReference{from: Some(s1.clone()),
        to: push1_func.clone()}, &(4 as usize));
    vd.append_external_event(ExternalEvent::PassByStaticReference{from: Some(s2.clone()),
        to: push1_func.clone()}, &(4 as usize));
    vd.append_external_event(ExternalEvent::PassByMutableReference{from: Some(s1.clone()),
        to: push2_func.clone()}, &(5 as usize));
    vd.append_external_event(ExternalEvent::PassByStaticReference{from: Some(s2.clone()),
        to: push2_func.clone()}, &(5 as usize));
    vd.append_external_event(ExternalEvent::PassByStaticReference{from: Some(s1.clone()),
        to: print_func.clone()}, &(6 as usize));

    vd.append_external_event(ExternalEvent::GoOutOfScope{ ro : s1.clone() }, 
        &(7 as usize));
    vd.append_external_event(ExternalEvent::GoOutOfScope{ ro : s2.clone() }, 
        &(7 as usize));

    //rendering image
    svg_generation::render_svg(&"examples/mutable_borrow_method_call/input/".to_owned(), &"examples/mutable_borrow_method_call/".to_owned(), &mut vd);
}