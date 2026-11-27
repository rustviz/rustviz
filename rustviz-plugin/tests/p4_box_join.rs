fn boxes(cond: bool) {
    {
        let mut b = Box::new(String::new());
        if cond {
            drop(*b);
        }
        b = Box::new(String::new());
        drop(b);
    }
    {
        let b = Box::new(String::new());
        drop(b);
    }
}

fn emptied() {
    {
        let mut b = Box::new(String::new());
        drop(*b);
        b = Box::new(String::new());
        drop(b);
    }
    { let b = Box::new(String::new()); drop(b); }
}
