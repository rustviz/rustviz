pub static CODE_PANEL_TEMPLATE : &'static str = 
"<svg width=\"{{tl_width}}px\" height=\"{{height}}px\" xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\">

    <desc>{{ visualization_name }}</desc>

    <defs>
        <style type=\"text/css\">
        <![CDATA[
        {{ css }}
        ]]>
        </style>
    </defs>

    <g>
        <text id=\"caption\" x=\"30\" y=\"50\">Hover over the visualization for more information.</text>
    </g>

    {{ code }}

</svg>";

pub static TIMELINE_PANEL_TEMPLATE: &'static str = 
"<svg width=\"{{tl_width}}px\" height=\"{{height}}px\" 
        xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" 
        id=\"{{tl_id}}\">

    <desc>{{ visualization_name }}</desc>

    <defs>
        <style type=\"text/css\">
        <![CDATA[
        {{ css }}
        
        text {
            user-select: none;
            -webkit-user-select: none;
            -moz-user-select: none;
            -ms-user-select: none;
        }
        ]]>
        </style>
        <!-- used when pass to function by ref -->
        <g id=\"functionDot\">
             <circle cx=\"0\" cy=\"0\" r=\"5\" fill=\"transparent\"/>
             <text class=\"functionIcon\" dx=\"-3.5\" dy=\"0\">f</text>
        </g>
        <marker id=\"arrowHead\" viewBox=\"0 0 10 10\"
            refX=\"0\" refY=\"4\"
            markerUnits=\"strokeWidth\"
            markerWidth=\"3px\" markerHeight=\"3px\"
            orient=\"auto\" class=\"arrow-head\">
            <path d=\"M 0 0 L 8.5 4 L 0 8 z\" fill=\"inherit\"/>
        </marker>
        <!-- glow highlight filter -->
        <filter id=\"glow\" x=\"-5000%\" y=\"-5000%\" width=\"10000%\" height=\"10000%\" filterUnits=\"userSpaceOnUse\">
            <feComposite in=\"flood\" result=\"mask\" in2=\"SourceGraphic\" operator=\"in\"></feComposite>
            <feGaussianBlur stdDeviation=\"2\" result=\"coloredBlur\"/>
            <feMerge>
                <feMergeNode in=\"coloredBlur\"></feMergeNode>
                <feMergeNode in=\"coloredBlur\"></feMergeNode>
                <feMergeNode in=\"coloredBlur\"></feMergeNode>
                <feMergeNode in=\"SourceGraphic\"></feMergeNode>
            </feMerge>
            <!-- increase brightness -->
            <feComponentTransfer>
                <feFuncR type=\"linear\" slope=\"2\"/>
                <feFuncG type=\"linear\" slope=\"2\"/>
                <feFuncB type=\"linear\" slope=\"2\"/>
            </feComponentTransfer>
        </filter>
    </defs>

    {{ diagram }}

</svg>";


pub static CSS_TEMPLATE: &'static str = 
"/* general setup */
    svg {
        background-color: var(--rv-bg);
    }
    
    text {
        vertical-align: baseline;
        text-anchor: start;
    }
    
    #heading {
        font-size: 24px;
        font-weight: bold;
    }
    
    #caption {
        fill: var(--rv-caption);
        font-size: 0.875em;
        font-family: \"Open Sans\", sans-serif;
        font-style: italic;
    }
    
    /* code related styling */
    text.code {
        fill: var(--rv-code);
        white-space: pre;
        font-family: \"Source Code Pro\", Consolas, \"Ubuntu Mono\", Menlo, \"DejaVu Sans Mono\", monospace, monospace !important;
        font-size: 0.875em;
    }

    text.code .kw {
        font-weight: bold;
    }

    text.code .comment {
        fill: var(--rv-comment);
        font-style: italic;
    }

    text.code .string {
        fill: var(--rv-string);
    }

    text.code .line-number {
        fill: var(--rv-line-number);
    }

    /* the line under the code a hovered element refers to (helpers.js) */
    .emph {
        stroke: var(--rv-underline);
    }

    text.label {
        font-family: \"Source Code Pro\", Consolas, \"Ubuntu Mono\", Menlo, \"DejaVu Sans Mono\", monospace, monospace !important;
        font-size: 0.875em;
    }
    
    /* timeline/event interaction styling */
    .solid {
        stroke-width: 5px;
    }
    
    .hollow {
        stroke-width: 1.5;
    }
    
    .dotted {
        stroke-width: 5px;
        stroke-dasharray: \"2 1\";
    }
    
    .extend {
        stroke-width: 1px;
        stroke-dasharray: \"2 1\";
    }

    /* a column while it is mutably lent out (#722): it can't be used
       until the borrow ends. A faint row of round dots, unlike the
       4px dashes of a branch arm that isn't running. */
    .lent {
        stroke-width: 2.5px;
        stroke-dasharray: 0 6;
        stroke-linecap: round;
        stroke-opacity: 0.6;
    }
    
    .functionIcon {
        paint-order: stroke;
        stroke-width: 3px;
        fill: var(--rv-bg);
        font-size: 20px;
        font-family: times;
        font-weight: lighter;
        dominant-baseline: central;
        text-anchor: start;
        font-style: italic;
    }
    
    .functionLogo {
        font-size: 20px;
        font-style: italic;
        paint-order: stroke;
        stroke-width: 3px;
        fill: var(--rv-bg) !important;
    }
    
    /* move, copy and call arrows */
    .arrow-shaft {
        stroke: var(--rv-arrow);
    }

    .arrow-head {
        fill: var(--rv-arrow);
    }

    /* flex related styling */
    .flex-container {
        display: flex;
        flex-direction: row;
        justify-content: flex-start;
        flex-wrap: nowrap;
        flex-shrink: 0;
    }
    
    object.tl_panel {
        flex-grow: 1;
    }
    
    object.code_panel {
        flex-grow: 0;
    }
    
    .tooltip-trigger {
        cursor: default;
    }
    
    .tooltip-trigger:hover{
        filter: url(#glow);
    }

    /* When the cursor is anywhere inside a branch column,
       glow every element grouped under .branch-group: one dashed or
       solid stretch of the arm, not the whole arm (#626).
       Single-element hover otherwise only highlighted one of
       the two parallel sides at a time, with no feedback on
       the transparent gap between them. */
    .branch-group:hover .tooltip-trigger {
        filter: url(#glow);
    }
";



/// Number of column colours (`--rv-c1`..`--rv-c12`, see `themes.rs`).
const COLUMN_COLOURS: u64 = super::themes::LIGHT.columns.len() as u64;

/// The `[data-hash="N"]` colour rules for hashes `0..=max_hash` (at
/// least the whole palette). Hash 0 is the code panel's colour for
/// function names (fill only, the code colour); every resource access
/// point and called function gets the next hash from 1 up, coloured
/// `--rv-c1`.. in order. Hashes past the palette reuse its colours
/// in order, so every column's state lines get a stroke however
/// many variables the program has. Without a rule a column's lines had
/// no stroke at all (#279). Only the colour cycles: `data-hash` itself
/// stays unique, since it's also what links code-panel names to their
/// columns on hover.
pub fn hash_css(max_hash: u64) -> String {
    let last = max_hash.max(COLUMN_COLOURS);
    let mut css = String::from("\n    /* hash based styling */\n");
    css.push_str("    [data-hash=\"0\"] { fill: var(--rv-code); }\n");
    for h in 1..=last {
        let c = (h - 1) % COLUMN_COLOURS + 1;
        css.push_str(&format!(
            "    [data-hash=\"{h}\"] {{ fill: var(--rv-c{c}); stroke: var(--rv-c{c}); }}\n"
        ));
    }
    css
}

/// Largest hash an SVG fragment refers to, as `data-hash="N"` or as a
/// function name's `hash="N"` (which the hover script copies into
/// `data-hash` to colour the name).
pub fn max_hash_in(svg: &str) -> u64 {
    svg.split("hash=\"")
        .skip(1)
        .filter_map(|rest| rest.split('"').next()?.parse::<u64>().ok())
        .max()
        .unwrap_or(0)
}
