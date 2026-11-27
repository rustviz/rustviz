fn make() -> String {
    String::from("made")
}

fn main() {
    let s = make();
    let s2 = String::from("hi");
    for i in 0..2 {
        if s2.len() > 1 {
            drop(s2);
            break;
        }
        let s4 = 'blk: {
            s.clone()
        };
    }
    let r2 = &s;
}
