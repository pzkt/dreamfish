mod support;

use support::expect_build_error;

#[test]
fn recursion_guard() {
    expect_build_error(
        "recursive",
        &[(
            "index.df",
            "<:A\n\
             #A\n\
             <div > <:B\n\
             #B\n\
             <span > <:A\n",
        )],
        "recursive component invocation detected",
    );
}

#[test]
fn prop_section_name_collision() {
    expect_build_error(
        "collision",
        &[(
            "index.df",
            "<:Widget\n\
             #Widget\n\
             <div > {item}\n\
             #item\n\
             <span > nope\n",
        )],
        "collides with a section",
    );
}

#[test]
fn duplicate_section_name() {
    expect_build_error(
        "duplicate",
        &[(
            "index.df",
            "#A\n\
             <div > a\n\
             #A\n\
             <span > b\n",
        )],
        "duplicate section name `#A`",
    );
}

#[test]
fn missing_referenced_df_file() {
    expect_build_error(
        "missing-file",
        &[("index.df", "<:./missing.df#Thing\n")],
        "does not exist",
    );
}

#[test]
fn else_without_if() {
    expect_build_error(
        "else-without-if",
        &[("index.df", "<:else\n")],
        "`<:else` without a matching `<:if`",
    );
}

#[test]
fn inconsistent_indentation() {
    expect_build_error(
        "bad-indent",
        &[("index.df", "<div\n   <p > two spaces\n  <span > one space\n")],
        "inconsistent indentation",
    );
}

#[test]
fn text_node_cannot_contain_children() {
    expect_build_error(
        "text-with-children",
        &[("index.df", "hello\n <b > bold\n")],
        "text node cannot contain child elements",
    );
}

#[test]
fn unresolved_reference() {
    expect_build_error(
        "unresolved",
        &[
            (
                "index.df",
                "<:Widget\n\
                 #Widget\n\
                 <div > {missing}\n",
            ),
        ],
        "unresolved reference `missing`",
    );
}