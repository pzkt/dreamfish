mod support;

use support::{build_fixture, count, Build};

#[test]
fn elements_attrs_nesting_text_and_br() {
    // elements, attributes, indentation nesting, sibling root nodes,
    // concatenated text lines, and a self-closing `<br`.
    let site = build_fixture("elements");
    assert_eq!(
        site.body("index.html"),
        "<div id=\"hero\" class=\"hero\"><h>Hello, world!</h>\
         <p class=\"sub\">Some text here.Another line.<br/>after the break.</p></div>\
         <footer><p>Bye</p></footer>"
    );
    assert!(!site.style("index.html").contains("none"));
}

#[test]
fn inline_chain_operator() {
    // `parent > child > grandchild`: exactly one child per step.
    let site = build_fixture("inline-chain");
    assert_eq!(
        site.body("index.html"),
        "<nav><ul><li><a href=\"/home\">Home</a></li></ul></nav>\
         <nav><ul><li><a href=\"/about\">About</a></li></ul></nav>"
    );
}

#[test]
fn text_and_attr_interpolation_of_props() {
    let site = build_fixture("text-interpolation");
    let body = site.body("index.html");
    assert!(body.contains("data-user=\"Ada\""));
    assert!(body.contains("Hello, Ada! friendly as always."));
}

#[test]
fn if_else_drives_both_branches() {
    let site = build_fixture("if-else");
    assert_eq!(
        site.body("index.html"),
        "<div class=\"card\"><span class=\"badge\">highlighted</span></div>\
         <div class=\"card\"><span class=\"plain\">plain</span></div>"
    );
}

#[test]
fn for_loop_over_kdl_data() {
    // iteration over a loaded data file, dotted field access, number
    // formatting (1 renders as `1`, 7.5 keeps its decimal).
    let site = build_fixture("for-data");
    assert_eq!(
        site.body("index.html"),
        "<ul><li>one: 1</li><li>two: 7.5</li></ul>\
         <div class=\"row\"><a href=\"/one\"><h>one</h><p>1</p></a></div>\
         <div class=\"row\"><a href=\"/two\"><h>two</h><p>7.5</p></a></div>"
    );
}

#[test]
fn kdl_tree_traversal() {
    // KDL nodes are traversable via dot notation; a node with a single
    // argument collapses to that argument, multiple arguments become a list;
    // `[n]` indexes the n-th argument (negative = from the end) and
    // `["key"]`/`[key]` reads a property or child by key.
    let site = build_fixture("kdl-tree");
    assert_eq!(
        site.body("index.html"),
        "<div>^4.17.21</div><div>1.2.0</div><div>2</div>\
         <ul><li>1.0.0</li><li>1.1.0</li><li>1.2.0</li><li>2</li></ul>\
         <div>1.0.0</div><div>1.0.0</div><div>^4.17.21</div>\
         <div>tsc</div><div>True</div><div>False</div>\
         <div>stable</div><div>1.0.0</div>"
    );
}

#[test]
fn kdl_simple_node_name_access() {
    // Two bare top-level nodes (`test 1`, `thing 2`) are addressed directly
    // by their node name, without a loop or an index.
    let site = build_fixture("kdl-simple");
    assert_eq!(
        site.body("index.html"),
        "<div>1</div><div>2</div>"
    );
}

#[test]
fn kdl_multiple_top_level_nodes() {
    // A KDL file may contain several root nodes: they become a list of node
    // contents, so `<:for>` iterates them and `[n]`/`[-n]` index across files.
    let site = build_fixture("kdl-multi");
    assert_eq!(
        site.body("index.html"),
        "<ul>\
         <li>prod = example.com on 443</li><li>staging = staging.example.com on 80</li>\
         </ul>\
         <div>staging.example.com</div><div>443</div><div>staging.example.com</div>"
    );
}

#[test]
fn local_components_with_props() {
    let site = build_fixture("components");
    assert_eq!(
        site.body("index.html"),
        "<div class=\"card\"><h>First</h><p>short</p></div>\
         <div class=\"card\"><h>Second</h><p>longer text</p></div>\
         <footer><p>made with dreamfish</p></footer>"
    );
}

#[test]
fn cross_file_component_with_transitive_assets() {
    // external component import; the library file is not emitted as a page;
    // its `.css`/`.js` asset sections follow the component transitively.
    let site = build_fixture("cross-file");
    assert!(site.body("index.html").contains(
        "<span class=\"shared\">loaded from lib</span>\
         <span class=\"shared\">loaded from lib</span>"
    ));
    assert!(!site.exists("lib/shared.html"));
    assert_eq!(count(&site.style("index.html"), ".shared { font-weight: bold; }"), 1);
    assert_eq!(count(&site.script("index.html"), "function announce(){ return 1; }"), 1);
}

#[test]
fn asset_bundling_dedup_and_external_files() {
    let site = build_fixture("assets");
    let style = site.style("index.html");
    let script = site.script("index.html");
    // in-file asset sections (rule 1), auto-included even without `<:` refs
    assert_eq!(count(&style, ".widget { color: red; }"), 1);
    assert_eq!(count(&script, "console.log(\"beep\");"), 1);
    // external raw files (rule 2)
    assert_eq!(count(&style, ".site { background: ivory; }"), 1);
    assert_eq!(count(&script, "window.siteReady = true;"), 1);
    // named asset section from another .df file (rule 2)
    assert_eq!(count(&script, "window.extra = true;"), 1);
    // `#Widget` invoked twice: assets still only appear once
    assert_eq!(count(&script, "console.log(\"beep\");"), 1);
}

#[test]
fn aliases_to_component_file_and_raw_asset() {
    let site = build_fixture("alias");
    let body = site.body("index.html");
    assert!(body.contains("<div class=\"coffee\">espresso</div>"));
    assert!(body.contains("<strong>SHOUT</strong>"));
    // alias to a KDL data file: traversed by node name, plus a loop over it
    assert!(body.contains(
        "<div class=\"site\">dreamfish — an indentation-based static site generator (v0.1)</div>\
         <ul><li>rust</li><li>kdl</li><li>dreamfish</li></ul>"
    ));
    assert!(site.script("index.html").contains("function logo(){ return \"svg\"; }"));
}

#[test]
fn script_block_comes_before_closing_body_tag() {
    // SPEC §8: the single `<script>` block is placed just before `</body>`.
    let site = build_fixture("assets");
    let html = site.read("index.html");
    let script_start = html.find("<script>").expect("script block present");
    let body_end = html.find("</body>").expect("body close present");
    assert!(
        script_start < body_end,
        "<script> must appear before `</body>`:\n{html}"
    );
}

fn assert_page(site: &Build, rel: &str, needle: &str) {
    let html = site.read(rel);
    assert!(
        html.contains(needle),
        "`{rel}` should contain `{needle}` but got:\n{html}"
    );
}

#[test]
fn pages_map_to_routes() {
    // index.df -> index.html, contact.df -> contact.html,
    // texts/blog.df -> texts/blog.html (data file resolved relative to texts/).
    let site = build_fixture("pages");
    assert_page(&site, "index.html", "Home page");
    assert_page(&site, "contact.html", "Contact us");
    assert_page(&site, "texts/blog.html", "First post");
    assert!(site.log.contains("Complete: built 3 pages from 3 files"), "log:\n{}", site.log);
    assert!(!site.log.contains("error"), "log:\n{}", site.log);
}