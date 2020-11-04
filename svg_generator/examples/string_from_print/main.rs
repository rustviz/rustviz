use rustviz_lib::data::{ExternalEvent, LifetimeTrait, ResourceAccessPoint, Owner, Function, VisualizationData, Visualizable};
use rustviz_lib::svg_frontend::svg_generation;
use std::collections::BTreeMap;

fn main() {
    let s = ResourceAccessPoint::Owner(Owner {
        hash: 1,
        name: String::from("s"),
        is_mut: false,
        lifetime_trait: LifetimeTrait::None
    });
    let from_func = ResourceAccessPoint::Function(Function {
        hash: 5,
        name: String::from("String::from()"),
    });
    let print_func = ResourceAccessPoint::Function(Function {
        hash: 6,
        name: String::from("println!()")
    });
    let mut vd = VisualizationData {
        timelines: BTreeMap::new(),
        external_events: Vec::new(),
        preprocess_external_events: Vec::new(),
        event_line_map: BTreeMap::new()
    };
    //
    // hash s : 1
    //
    vd.append_external_event(ExternalEvent::Move{from: Some(from_func.clone()),
        to: Some(s.clone())}, &(2 as usize));
    vd.append_external_event(ExternalEvent::PassByStaticReference{from: Some(s.clone()),
        to: Some(print_func.clone())}, &(3 as usize));
    vd.append_external_event(ExternalEvent::GoOutOfScope{ ro: s.clone() }, &(4 as usize));

    svg_generation::render_svg(&"examples/string_from_print/input/".to_owned(), &"examples/string_from_print/".to_owned(), & mut vd);
}