fn read(_: &String) {}

fn run(opt: Option<()>) {
    let s = String::from("outer");
    let r = &s;
    'scope: {
        let Some(_) = opt else { read(r); break 'scope; };
    }
}
