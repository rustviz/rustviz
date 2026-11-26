#![feature(try_blocks)]
use std::ops::ControlFlow;

fn run(input: ControlFlow<()>) {
    let outer = String::from("outer");
    let late;
    let result: ControlFlow<()> = try {
        late = String::from("late");
        let inner = String::from("inner");
        input?;
        drop(inner);
    };
    drop(late);
    drop(outer);
    let _ = result;
}

fn nested(first: Option<()>, second: Option<()>) {
    let keep = String::new();
    let _: Option<()> = try {
        let middle = String::new();
        let _: Option<()> = try {
            let deepest = String::new();
            first?;
            drop(deepest);
        };
        second?;
        drop(middle);
    };
    drop(keep);
}

fn loops(inputs: Vec<Option<()>>, next: Option<()>) {
    let mut iter = inputs.into_iter();
    for item in &mut iter {
        let carried = String::new();
        let _: Option<()> = try {
            let in_loop = String::new();
            item?;
            drop(in_loop);
        };
        drop(carried);
    }

    let nested_iter = vec![String::new()].into_iter();
    let _: Option<()> = try {
        for element in nested_iter {
            next?;
            drop(element);
        }
    };
}

fn main() {
    run(ControlFlow::Break(()));
    nested(None, None);
    loops(vec![None], None);
}
