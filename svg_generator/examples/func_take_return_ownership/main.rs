use rustviz_lib::data::{ExternalEvent, LifetimeTrait, ResourceAccessPoint, Owner, MutRef, StaticRef, Function, VisualizationData, Visualizable};
use rustviz_lib::svg_frontend::svg_generation;
use std::collections::BTreeMap;
fn main() {
    let s = ResourceAccessPoint::Owner(Owner {
        hash: 1,
        name: String::from("s"),
        is_mut: true,
        lifetime_trait: LifetimeTrait::Move,
    });
    let some_string = ResourceAccessPoint::Owner(Owner {
        hash: 2,
        name: String::from("some_string"),
        is_mut: false,
        lifetime_trait: LifetimeTrait::Move,
    });
    let string_ctor = Some(ResourceAccessPoint::Function(Function {
        hash: 5,
        name: String::from("String::from()"),
    }));
    let take_return_func = Some(ResourceAccessPoint::Function(Function {
        hash: 6,
        name: String::from("take_and_return_ownership()"),
    }));
    let print_func = Some(ResourceAccessPoint::Function(Function {
        hash: 7,
        name: String::from("println!()"),
    }));
    let mut vd = VisualizationData {
        timelines: BTreeMap::new(),
        external_events: Vec::new(),
        preprocess_external_events: Vec::new(),
        event_line_map: BTreeMap::new()
    };

    vd.append_external_event(ExternalEvent::Move{from: string_ctor.clone(),
        to: Some(s.clone())}, &(8 as usize));
    vd.append_external_event(ExternalEvent::Move{ from : Some(s.clone()),
        to: take_return_func.clone()}, &(9 as usize));
    vd.append_external_event(ExternalEvent::Move{ from : take_return_func.clone(),
        to: Some(s.clone())}, &(9 as usize));
    vd.append_external_event(ExternalEvent::GoOutOfScope{ ro : s.clone() }, 
        &(11 as usize));

    vd.append_external_event(ExternalEvent::InitializeParam{param: some_string.clone()},
        &(2 as usize));
    vd.append_external_event(ExternalEvent::Move{ from : Some(some_string.clone()),
        to: None}, &(5 as usize));

    //rendering image
    svg_generation::render_svg(&"examples/func_take_return_ownership/input/".to_owned(), &"examples/func_take_return_ownership/".to_owned(), &mut vd);
}
