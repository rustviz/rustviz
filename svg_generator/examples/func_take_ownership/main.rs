use rustviz_lib::data::{ExternalEvent, LifetimeTrait, ResourceAccessPoint, Owner, Function, Visualizable, VisualizationData};
use rustviz_lib::svg_frontend::svg_generation;
use std::collections::BTreeMap;

fn main() {
    // Variables
    let s = ResourceAccessPoint::Owner(Owner {
        hash: 1,
        name: String::from("s"),
        is_mut: false,
        lifetime_trait: LifetimeTrait::Move,
    });
    let some_string = ResourceAccessPoint::Owner(Owner {
        hash: 2,
        name: String::from("some_string"),
        is_mut: false,
        lifetime_trait: LifetimeTrait::Move,
    });
    // Functions
    let from_func = ResourceAccessPoint::Function(Function {
        hash: 5,
        name: String::from("String::from()"),
    });
    let takes_ownership = ResourceAccessPoint::Function(Function {
        hash: 6,
        name: String::from("takes_ownership()"),
    });
    let println_func = ResourceAccessPoint::Function(Function {
        hash: 8,
        name: String::from("println!()"),
    });
    let mut vd = VisualizationData {
        timelines: BTreeMap::new(),
        external_events: Vec::new(),
    };

    // let s = String::from("hello");
    vd.append_external_event(ExternalEvent::Move{from: Some(from_func.clone()),
        to: Some(s.clone())}, &(2 as usize));
    // takes_ownership(s);
    vd.append_external_event(ExternalEvent::Move{from: Some(s.clone()),
        to: Some(takes_ownership.clone())}, &(3 as usize));

    // fn takes_ownership(some_string: String) {
    vd.append_external_event(ExternalEvent::Move{from: None,
        to: Some(some_string.clone()) }, &(7 as usize));
    // println!("{}", some_string);
    vd.append_external_event(ExternalEvent::PassByStaticReference{
        from: Some(some_string.clone()), to: Some(println_func.clone()) },
        &(8 as usize));
    vd.append_external_event(ExternalEvent::GoOutOfScope{ ro: some_string },
        &(9 as usize));

    // Out of Scope
    vd.append_external_event(ExternalEvent::GoOutOfScope{ ro: s }, &(5 as usize));

    //rendering image
    svg_generation::render_svg(&"examples/func_take_ownership/input/".to_owned().to_owned(),
        &"examples/func_take_ownership/".to_owned(), &vd);
}