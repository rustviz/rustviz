// #941: `**rb = …` with `rb: &mut Box<String>` replaces the String in
// bx's Box; bx keeps its Box (and heap allocation), and only the old
// String is dropped. bx read "bx acquires ownership of a new resource;
// its previous resource (including the String it holds) is dropped",
// and rb named the place `*rb` (the Box). The same holds through two
// references (`***rrb`) and in a function (`**p`). Checked with a
// `Drop`-printing type.

fn set(p: &mut Box<String>) {
    **p = String::from("p");
}

fn main() {
    let mut bx = Box::new(String::from("q"));
    let rb = &mut bx;
    **rb = String::from("z");
    println!("{}", bx);

    let mut cx = Box::new(String::from("c"));
    let mut rc = &mut cx;
    let rrc = &mut rc;
    ***rrc = String::from("c2");
    println!("{}", cx);

    let mut dx = Box::new(String::from("d"));
    set(&mut dx);
    println!("{}", dx);
}
