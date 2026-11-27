type Route = for<'a> fn((&'a mut String, &'a mut String)) -> &'a mut String;

fn first<'a>((a, _): (&'a mut String, &'a mut String)) -> &'a mut String { a }
fn second<'a>((_, b): (&'a mut String, &'a mut String)) -> &'a mut String { b }
fn take(w: &mut String) { w.push('!'); }

fn with_else(o: Option<i32>, route: Route) -> (String, String) {
    let mut v = String::from("v");
    let mut t = String::from("t");
    let r = &mut v;
    if let Some(x) = o && {
        let inputs = (r, &mut t);
        let wr = route(
            inputs,
        );
        take(wr);
        true
    } && x > 5 {
        v.push('Y');
    } else {
        v.push('N');
    }
    (v, t)
}

fn main() {
    for seed in [None, Some(1), Some(6)] {
        let a = with_else(seed, first);
        let b = with_else(seed, second);
        println!("{seed:?}, first: {a:?}; second: {b:?}");
    }
}
