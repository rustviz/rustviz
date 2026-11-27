fn make() -> String { String::from("live") }
fn pair() -> (String, String) { (make(), make()) }
struct P { name: String, tag: String }
fn make_p() -> P { P { name: make(), tag: make() } }

fn nested_pair(z: String) {
    match (z, ((pair(), 0), 0)) {
        (_, (((s, _), _), _)) => drop(s),
    }
}

fn nested_struct(z: Option<String>) {
    if let (Some(c), (P { name, .. }, _)) = (z, (make_p(), 0)) {
        println!("{c} {name}");
    }
}

fn mutable_temp() {
    let x = &mut make();
    x.push('!');
    println!("{x}");
}

enum Payload { One(String), Two(String, String), Other }
fn payload(two: bool) -> Payload {
    if two { Payload::Two(make(), make()) } else { Payload::One(make()) }
}

fn continuing(two: bool) {
    match payload(two) {
        Payload::One(x) | Payload::Two(x, _) => {
            println!("{x}");
            drop(x);
        }
        Payload::Other => {}
    }
}

fn before_abort(a: bool, s: String) {
    let r = if a { &make() } else { &s };
    println!("{r}");
    std::process::abort();
}

fn early_return(c: bool) {
    let r = if c { &make() } else { return; };
    println!("{r}");
}

struct Bomb;
impl Drop for Bomb {
    fn drop(&mut self) { panic!("boom"); }
}
fn diverges() -> ! {
    { let _bomb = Bomb; }
    loop {}
}
fn indirect(f: fn() -> !) {
    let live = &make();
    println!("{live}");
    f();
}
fn implicit() {
    let live = &make();
    println!("{live}");
    diverges();
}
fn overwrite(p: &mut Bomb) {
    let live = &make();
    *p = Bomb;
    println!("{live}");
}
fn constructing() {
    let earlier = &make();
    let _ = pair();
    println!("{earlier}");
}

fn sink<T>(_: T) {}
fn noncopy(p: &mut u8) { sink(std::convert::identity(p)); }
fn building_array(f: fn() -> String) -> [String; 2] {
    [make(), f()]
}
