fn read(_: &String) {}

fn run(a: &String, b: &String, ready: Option<()>, left: bool) -> Option<()> {
    let r = if left { a } else { b };
    let (copy, last) = (r, r);
    ready?;
    if left {
        read(last);
    } else {
        read(copy);
    }
    Some(())
}

fn pattern(mut o: Option<String>, use_it: bool) {
    let Some(first) = o.as_mut() else {
        let check = o.as_ref();
        return;
    };
    if use_it {
        first.push('!');
    } else {
        let check = o.as_ref();
    }
}
