use rustviz_lib::data::{ExternalEvent, LifetimeTrait, ResourceAccessPoint, Owner, MutRef, StaticRef, Function, VisualizationData, Visualizable};
use rustviz_lib::svg_frontend::svg_generation;
use std::collections::BTreeMap;
fn main() {
    let six = ResourceAccessPoint::Owner(Owner {
        hash: 1,
        name: String::from("six"),
        is_mut: false,
        lifetime_trait: LifetimeTrait::Copy,
    });
    let x = ResourceAccessPoint::Owner(Owner {
        hash: 2,
        name: String::from("x"),
        is_mut: false,
        lifetime_trait: LifetimeTrait::Copy,
    });
    let plus_one_func = ResourceAccessPoint::Function(Function {
        hash: 5,
        name: String::from("plus_one()"),
    });
    let mut vd = VisualizationData {
        timelines: BTreeMap::new(),
        external_events: Vec::new(),
        preprocess_external_events: Vec::new(),
        event_line_map: BTreeMap::new()
    };

    vd.append_external_event(ExternalEvent::Move{from: Some(plus_one_func.clone()),
        to: Some(six.clone())}, &(2 as usize));
    vd.append_external_event(ExternalEvent::GoOutOfScope{ ro : six.clone() }, 
        &(3 as usize));

    //rendering image
    svg_generation::render_svg(&"examples/function/input/".to_owned(), &"examples/function/".to_owned(), &mut vd);
}