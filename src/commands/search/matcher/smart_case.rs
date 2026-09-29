//! Smart-case: which patterns spell an UPPERCASE letter.
//!
//! The decision reads the PARSED pattern's literal characters, the rule ripgrep's own
//! smart-case applies: a letter the pattern spells counts, bare or inside a bracket class
//! (both ends of a range included), and an escape that names a class or an assertion does
//! not, because `\S`, `\W`, `\D` and `\B` spell no letter to match. A scan of the pattern's
//! characters reads the `S` of `\S` as an uppercase letter and turns a search the reader
//! wrote in lowercase case-sensitive. The HIR is the wrong layer for this: it has already
//! expanded `\S` into a class holding `A-Z`, so the two can no longer be told apart there.

use regex_syntax::ast::{self, Ast, ClassSetItem};

/// True when the pattern spells an uppercase letter the regex would match literally.
///
/// `[A-Z]` counts: its ends are letters the pattern wrote, so the reader asked for case. A
/// pattern the AST parser refuses falls back to the character scan, and the compile step
/// reports the syntax error either way.
pub(crate) fn pattern_has_uppercase_literal(pattern: &str) -> bool {
    match ast::parse::Parser::new().parse(pattern) {
        Ok(tree) => match ast::visit(&tree, Uppercase(false)) {
            Ok(found) => found,
            Err(never) => match never {},
        },
        Err(_) => pattern.chars().any(char::is_uppercase),
    }
}

/// Folds every literal character of the tree into one flag.
#[derive(Debug)]
struct Uppercase(bool);

impl ast::Visitor for Uppercase {
    type Output = bool;
    type Err = std::convert::Infallible;

    fn finish(self) -> Result<bool, Self::Err> {
        Ok(self.0)
    }

    fn visit_pre(&mut self, node: &Ast) -> Result<(), Self::Err> {
        if let Ast::Literal(lit) = node {
            self.0 |= lit.c.is_uppercase();
        }
        Ok(())
    }

    fn visit_class_set_item_pre(&mut self, item: &ClassSetItem) -> Result<(), Self::Err> {
        match item {
            ClassSetItem::Literal(lit) => self.0 |= lit.c.is_uppercase(),
            ClassSetItem::Range(range) => {
                self.0 |= range.start.c.is_uppercase() || range.end.c.is_uppercase();
            }
            _ => {}
        }
        Ok(())
    }
}
