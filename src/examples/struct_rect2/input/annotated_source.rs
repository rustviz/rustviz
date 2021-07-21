struct Rectangle {
    <tspan data-hash="2">width</tspan>: u32,
    <tspan data-hash="3">height</tspan>: u32,
}

impl Rectangle {
    fn <tspan class="fn" data-hash="0" hash="5">area</tspan>(<tspan data-hash="8">&amp;self</tspan>) -> u32 {
        <tspan data-hash="8">self</tspan>.<tspan data-hash="2">width</tspan> * <tspan data-hash="8">self</tspan>.<tspan data-hash="3">height</tspan>
    }
}

fn <tspan class="fn" data-hash="0" hash="6">print_area</tspan>(<tspan data-hash="4">rect</tspan>: &amp;Rectangle) -> u32 {
    <tspan class="fn" data-hash="0" hash="7">println!</tspan>(
        "The area of the rectangle is {} square pixels.",
        <tspan data-hash="4">rect</tspan>.<tspan class="fn" data-hash="0" hash="5">area</tspan>() // dot even though it's actually a reference
    );
}

fn main() {
    let <tspan data-hash="1">r</tspan> = Rectangle {
        <tspan data-hash="2">width</tspan>: 30,
        <tspan data-hash="3">height</tspan>: 50,
    };

    <tspan class="fn" data-hash="0" hash="6">print_area</tspan>(<tspan data-hash="1">&amp;r</tspan>);
}