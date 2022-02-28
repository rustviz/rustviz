struct Rect {
    <tspan data-hash="2">w</tspan>: u32,
    <tspan data-hash="3">h</tspan>: u32,
}

fn main() {
    let <tspan data-hash="1">r</tspan> = Rect {
        <tspan data-hash="2">w</tspan>: 30,
        <tspan data-hash="3">h</tspan>: 50,
    };

    <tspan class="fn" data-hash="0" hash="6">println!</tspan>(
        "The area of the rectangle is {} square pixels.",
        <tspan class="fn" data-hash="0" hash="5">area</tspan>(<tspan data-hash="1">&amp;r</tspan>)
    );
    
    <tspan class="fn" data-hash="0" hash="6">println!</tspan>("The height of that is {}.", r.h);
}

fn <tspan class="fn" data-hash="0" hash="5">area</tspan>(<tspan data-hash="4">rect</tspan>: &amp;Rect) -&gt; u32 {
    rect.w * rect.h
}