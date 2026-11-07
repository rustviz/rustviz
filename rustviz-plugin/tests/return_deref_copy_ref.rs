fn first<'a>(r: &&'a String) -> &'a String {
    *r
}

fn twice() -> i32 {
    let x = 5;
    let r = &x;
    let y = &r;
    **y
}

fn main() {
    let s = String::from("hi");
    let t = &s;
    let u = first(&t);
    println!("{} {}", u, twice());
}
