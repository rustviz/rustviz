struct H<'a> {
    r: &'a String,
}

fn main() {
    let s = String::from("s");
    let r = &s;
    let c = true;
    let d = false;
    if c {
        let h = H { r };
        if d {
            drop(h);      // line 13
        } else {
            drop(h);      // line 15
        }
    }
    println!("{}", r);
}
