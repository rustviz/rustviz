struct P { name: String, age: String }

fn main() {
    let s = String::from("s");
    let t = String::from("t");
    let (ref a, ..) = (s, t);
    let c = String::from("c");
    let d = String::from("d");
    let (_, ref e) = (c, d);
    let f = String::from("f");
    let g = String::from("g");
    let h = String::from("h");
    let (ref i, _, ..) = (f, g, h);
    let m = String::from("m");
    let n = String::from("n");
    let (x, _) = (m, n);
    let q = String::from("q");
    let r = String::from("r");
    let P { ref name, .. } = P { name: q, age: r };
    let u = String::from("u");
    let w = String::from("w");
    let [ref k, _, ..] = [u, w, String::new()];
    let z = String::from("z");
    let _ = z;
    println!("{} {} {} {} {} {} {}", a, e, i, x, name, k, z);
}
