fn two<'a, 'b>(c: &'a String, d: &'b String) -> (&'a String, &'b String) { (c, d) }
fn view(s: &mut String) -> &str { s }
fn refs(mut a: String, mut b: String, c: String, mut d: String) {
    {
        let (x, y) = two(&c, &d);
        drop(y.len());
        d.clear();
        drop(x.len());
        let r = view(&mut a);
        drop(r.len());
    }
    {
        let (x, y) = two(&c, &d);
        drop(y.len());
        d.clear();
        drop(x.len());
        let r = view(&mut b);
        drop(r.len());
    }
}

fn deferred(cond: bool, first: String, second: String) {
    let selected;
    selected = 'choice: {
        if cond {
            let r = &first;
            break 'choice r;
        }
        let r = &second;
        r
    };
    println!("{selected}");
}
