// An `else` arm with no events for `s`: its column is solid over the
// `else` body and dashed only beside the `if` body (#843).
fn takes(s: String) {
    println!("{}", s);
}

fn main() {
    let s = String::from("hi");
    let flag = true;
    if flag {
        takes(s);
        println!("moved");
    } else {
        println!("kept");
        println!("still kept");
    }
}
