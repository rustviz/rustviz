// #136: methods on a unit struct. `fn make() -> Marker { Marker }`
// (a unit struct constructor as a function's result) used to panic
// in the renderer.
struct Marker;

impl Marker {
    fn name(&self) -> &'static str { "marker" }
    fn make() -> Marker { Marker }
}

fn main() {
    let m = Marker;
    let n = Marker::make();
    println!("{} {}", m.name(), n.name());
}
