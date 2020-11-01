use rustviz_lib::data::{ExternalEvent, LifetimeTrait, ResourceAccessPoint, Owner, Function, Visualizable, VisualizationData};
use rustviz_lib::svg_frontend::svg_generation;
use std::collections::BTreeMap;

fn main() {
    // Variables
    let x = ResourceAccessPoint::Owner(Owner {
        hash: 1,
        name: String::from("x"),
        is_mut: false,
        lifetime_trait: LifetimeTrait::Move
    });
    let y = ResourceAccessPoint::Owner(Owner {
        hash: 2,
        name: String::from("y"),
        is_mut: false,
        lifetime_trait: LifetimeTrait::Move
    });
    // Functions
    let from_func = ResourceAccessPoint::Function(Function {
        hash: 5,
        name: String::from("String::from()"),
    });
    let print_func = ResourceAccessPoint::Function(Function {
        hash: 6,
        name: String::from("println!()"),
    });

    let mut vd = VisualizationData {
        timelines: BTreeMap::new(),
        external_events: Vec::new()
    };

    // let s = String::from("hello");
    vd.append_external_event(ExternalEvent::Move{from: Some(from_func.clone()),
        to: Some(x.clone())}, &(2 as usize));
    // let y = x;
    vd.append_external_event(ExternalEvent::Move{from: Some(x.clone()),
        to: Some(y.clone())}, &(3 as usize));
    vd.append_external_event(ExternalEvent::PassByStaticReference{from: Some(y.clone()),
        to: Some(print_func.clone())}, &(4 as usize));
    // Out of Scope
    vd.append_external_event(ExternalEvent::GoOutOfScope{ ro: x }, &(5 as usize));
    vd.append_external_event(ExternalEvent::GoOutOfScope{ ro: y }, &(5 as usize));

    // rendering image
    svg_generation::render_svg(&"examples/string_from_move_print/input/".to_owned().to_owned(), &"examples/string_from_move_print/".to_owned(), &vd);
}