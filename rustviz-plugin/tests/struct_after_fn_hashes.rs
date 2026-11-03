// #231: a struct item between two fns. Annotating the struct's fields
// used to wrap the plugin's RAP hash counter with `% 10`, so main's
// s and t got the hashes of helper's b and c: one column each for
// b/s and c/t, labelled b and c, with b's and c's tooltips. Which
// fn's line the shared labels sat on depended on HashMap order, so
// the SVG also changed from run to run.
fn helper() {
    let a = 1;
    let b = 2;
    let c = 3;
    let d = 4;
    let e = 5;
    let f = 6;
    let g = 7;
    let h = 8;
    let i = 9;
}

struct Point {
    x: i32,
}

fn main() {
    let s = String::from("hi");
    let t = s;
    println!("{}", t);
}
