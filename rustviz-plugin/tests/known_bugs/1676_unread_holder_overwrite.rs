// Known bug, from the closed 2.x fix PRs (cx/oracle1-1671-1672-1673-1676); kept as a RustViz 3 regression case.
// Issue(s): #1676 (see known_bugs/README.md for what main draws wrong).
struct H<'a> { r: &'a String }
fn main() {
    let mut a = String::from("a");
    let b = String::from("b");
    let mut hold = H { r: &a };
    let other = H { r: &b };
    hold = other;
    a.push('!');
    println!("{}", hold.r);
}

struct Reads<'a> { r: &'a String }
impl Drop for Reads<'_> {
    fn drop(&mut self) { println!("{}", self.r); }
}

fn read_by_drop() {
    let first = String::from("first");
    let next = String::from("next");
    let mut h = Reads { r: &first };
    println!("{}", next);
    h = Reads { r: &next };
    println!("{}", h.r);
}
