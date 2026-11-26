fn make(_a: u8, _b: u8) -> String { String::new() }

fn f4(o: Option<u8>, p: Option<u8>) -> Option<u8> {
    let mut s = String::from("a");
    println!("{}", s);
    s = make(o?, p?);
    println!("{}", s);
    Some(1)
}

fn before_and_after(a: Option<u8>, b: Option<u8>, c: Option<u8>) -> Option<()> {
    let mut text = String::from("old");
    println!("{text}");
    a?; text = b?.to_string(); c?;
    println!("{text}");
    Some(())
}

fn operand_block(o: Option<u8>) -> Option<()> {
    let mut held = String::from("old");
    println!("{held}");
    { held = String::new(); o }?;
    println!("{held}");
    Some(())
}

fn inside_loop(a: Option<u8>, b: Option<u8>) -> Option<()> {
    let items = vec![String::from("item")];
    let iter = items.into_iter();
    for item in iter {
        let local = item;
        a?; b?;
        println!("{local}");
    }
    Some(())
}

fn tuple_scrutinee(a: Option<u8>, b: Option<u8>) -> Option<()> {
    let first = String::from("first");
    let rest = String::from("rest");
    match (first, rest) { (head, _) => { a?; b?; println!("{head}"); } }
    Some(())
}

fn alongside_closure(a: Option<u8>, b: Option<u8>, c: Option<u8>) -> Option<()> {
    let closure_line = String::from("held");
    let _run = || Some(a?); b?; c?;
    println!("{closure_line}");
    Some(())
}
