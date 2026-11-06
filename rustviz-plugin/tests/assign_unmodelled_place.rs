// Assignments whose target place has no column of its own: a field
// through `&mut self`, a field through a local `&mut`, an index. These
// used to crash the plugin, and the value stored there must still be
// shown as moved (not dropped at scope end). The field writes are drawn
// on the reference's column (#147); the index write isn't drawn.
struct User {
    name: String,
    visits: u32,
}

impl User {
    fn rename(&mut self, a: String) {
        self.name = a;
        self.visits += 1;
    }
}

fn main() {
    let mut u = User { name: String::from("x"), visits: 0 };
    u.rename(String::from("y"));

    let b = String::from("z");
    let r = &mut u;
    r.name = b;

    let mut v = vec![String::from("p")];
    let c = String::from("q");
    v[0] = c;
    println!("{} {:?}", u.name, v);
}
