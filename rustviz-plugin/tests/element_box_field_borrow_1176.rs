struct P { name: String, age: u32 }
fn grow(s: &mut String) { s.push('!'); }
fn main() {
    let mut people = vec![P { name: String::from("a"), age: 1 }];
    let age = &mut people[0].age;
    *age += 1;
    grow(&mut people[0].name);
    let first = &people[0].name;
    println!("{}", first);
    people[0].name.push('x');
    let n = people[0].name.len();
    let mut b = Box::new(P { name: String::from("b"), age: 2 });
    let a = &mut b.age;
    *a += 1;
    b.name.push('y');
    grow(&mut b.name);
    println!("{} {}", b.age, n);
    let mut v = vec![1, 2];
    let e = &mut v[0];
    *e += 1;
    println!("{:?}", v);
}
