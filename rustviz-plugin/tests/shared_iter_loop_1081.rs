struct Inv {
    items: Vec<String>,
}
impl Inv {
    fn show(&self) {
        for it in self.items.iter() {
            println!("{}", it);
        }
    }
}
fn total(v: &Vec<i32>) -> i32 {
    let mut t = 0;
    for e in v.iter() {
        t += *e;
    }
    t
}
fn pairs(a: &Vec<i32>, w: &Vec<i32>) {
    for (x, y) in a.iter().zip(w.iter()) {
        println!("{} {}", x, y);
    }
}
fn largest(list: &[i32]) -> i32 {
    let mut m = list[0];
    for &item in list.iter() {
        if item > m {
            m = item;
        }
    }
    m
}
fn main() {
    let inv = Inv { items: vec![String::new()] };
    inv.show();
    let mut p = vec![1, 2];
    let r = &p;
    for n in r.iter() {
        println!("{}", n);
    }
    p.push(3);
    total(&p);
    pairs(&p, &p);
    largest(&p);
    let q = &mut p;
    for k in q.iter().map(|k| k * 2).collect::<Vec<i32>>() {
        q.push(k);
    }
}
