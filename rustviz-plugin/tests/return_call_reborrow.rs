// References returned through a reference parameter other than by
// `&v[0]` (#1078). `v.get_mut(0).unwrap()` returns a reference into the
// Vec v points to: get_mut's signature gives its result the lifetime of
// the borrow of `*v`, so that borrow goes on in the caller instead of
// ending in the function. `&mut *w` is an explicit reborrow of all of
// what w points to; w isn't moved. `&rows[0][1]` points into the Vec
// the reference `rows[0]` points to, with that reference's own lifetime
// `'a`, so it is reborrowed through `rows[0]`, not through rows.
fn first_mut(v: &mut Vec<i32>) -> &mut i32 {
    v.get_mut(0).unwrap()
}

fn first(xs: &Vec<i32>) -> &i32 {
    xs.first().unwrap()
}

fn smallest(ys: &Vec<i32>) -> &i32 {
    ys.iter().min().unwrap()
}

fn whole(w: &mut Vec<i32>) -> &mut Vec<i32> {
    &mut *w
}

fn whole_shared(u: &Vec<i32>) -> &Vec<i32> {
    &*u
}

fn inner<'a>(rows: &Vec<&'a Vec<i32>>) -> &'a i32 {
    &rows[0][1]
}

// `copied` gives back the `&'a str` stored in the Vec, not a reference
// into it: the borrow of what names points to ends here.
fn copied_out<'a>(names: &Vec<&'a str>) -> &'a str {
    names.first().copied().unwrap()
}

struct Bag {
    items: Vec<String>,
}

impl Bag {
    fn top(&self) -> &String {
        self.items.first().unwrap()
    }
}

fn main() {
    let mut v = vec![1, 2, 3];
    let a = first_mut(&mut v);
    *a += 1;
    let b = first(&v);
    let c = smallest(&v);
    println!("{} {}", b, c);
    let d = whole(&mut v);
    d.push(4);
    let e = whole_shared(&v);
    println!("{:?}", e);
    let row = vec![5, 6];
    let rows = vec![&row];
    let f = inner(&rows);
    println!("{}", f);
    let s = String::from("hi");
    let names = vec![s.as_str()];
    let g = copied_out(&names);
    println!("{}", g);
    let bag = Bag { items: vec![String::from("x")] };
    let t = bag.top();
    println!("{}", t);
}
