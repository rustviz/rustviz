fn run(branch: bool, stop: bool, mut o: Option<Vec<String>>) {
    if branch {
        for e in &mut o {
            if stop {
                e.push(String::from("break"));
                break;
            }
            e.push(String::from("continue"));
        }
        o.take();
    } else {
        o.take();
    }
}
