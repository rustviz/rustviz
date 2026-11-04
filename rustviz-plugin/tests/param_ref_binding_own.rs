struct P { name: String, age: u32 }

fn whole(ref s: String) -> usize {
    s.len()
}

fn part((ref a, b): (String, i32)) -> usize {
    a.len() + b as usize
}

fn grow(ref mut v: Vec<i32>) -> usize {
    v.push(1);
    v.len()
}

fn field(P { ref name, age }: P) -> usize {
    name.len() + age as usize
}

fn shared(&(ref x, y): &(String, i32), (w, z): &(String, i32)) -> usize {
    x.len() + y as usize + w.len() + *z as usize
}

fn main() {
    let s1 = String::from("a");
    let n1 = whole(s1);
    let t = (String::from("b"), 2);
    let n2 = part(t);
    let v1 = vec![1, 2];
    let n3 = grow(v1);
    let p = P { name: String::from("c"), age: 3 };
    let n4 = field(p);
    let q = (String::from("d"), 4);
    let n5 = shared(&q, &q);
    println!("{} {} {} {} {}", n1, n2, n3, n4, n5);
}
