enum Guard { First, Last }

impl Drop for Guard {
    fn drop(&mut self) {
        if matches!(self, Self::First) {
            eprintln!("drop first");
            panic!("destructor panicked");
        }
        eprintln!("drop last");
    }
}

fn double_panic() {
    let last = Guard::Last;
    let _v = vec![Guard::First, (|| -> Guard { panic!("element panicked") })(), last];
}

fn abort_element() {
    let after_abort = String::from("s");
    let _v = vec![format!("first"), (|| -> String { std::process::abort() })(), after_abort];
}

fn branch_elements(flag: bool) {
    let after_branch = String::from("s");
    let a = || -> String { panic!("first branch") };
    let b = || -> String { panic!("second branch") };
    let c = || -> String { panic!("unreachable element") };
    let _v = vec![
        if flag {
            a()
        } else {
            b()
        },
        c(),
    ];
    loop {}
}

fn async_element() {
    let _future = async {
        let after_panic = String::from("s");
        let _v = vec![format!("first"), (|| -> String { panic!("async element") })(), after_panic];
    };
}
