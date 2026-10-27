//! The shorthand signature string form.
//!
//! Input names, an arrow, then output names. Each name may carry a type after
//! a colon, and an array type may name its element type in brackets:
//!
//! ```text
//! question -> answer
//! document, style -> summary: string, confidence: number
//! text -> tags: array[string]
//! ```
//!
//! Type names are not checked here. They are resolved against the codec
//! registry when the signature is built, which is what lets a shorthand
//! string use a type registered by another crate.

use crate::error::SignatureError;
use crate::field::{Field, Kind};

/// The separator between inputs and outputs.
pub const ARROW: &str = "->";

/// Parses a shorthand string into its input and output fields.
///
/// # Errors
///
/// Returns [`SignatureError::Definition`] when the string has no arrow or
/// more than one, when either side is empty, or when a field declaration is
/// malformed.
///
/// # Examples
///
/// ```
/// use cosmos_llm_signature::parser::parse;
///
/// let (inputs, outputs) = parse("text -> tags: array[string]").unwrap();
/// assert_eq!(inputs[0].name, "text");
/// assert_eq!(outputs[0].of.as_deref(), Some("string"));
/// ```
pub fn parse(text: &str) -> Result<(Vec<Field>, Vec<Field>), SignatureError> {
    let mut sides = text.split(ARROW);
    let (Some(left), Some(right), None) = (sides.next(), sides.next(), sides.next()) else {
        return Err(SignatureError::Definition(format!(
            "signature string must contain exactly one '{ARROW}', got {text:?}"
        )));
    };

    let inputs = parse_side(left, Kind::Input)?;
    let outputs = parse_side(right, Kind::Output)?;
    if inputs.is_empty() {
        return Err(SignatureError::Definition(format!(
            "signature string {text:?} declares no input fields"
        )));
    }
    if outputs.is_empty() {
        return Err(SignatureError::Definition(format!(
            "signature string {text:?} declares no output fields"
        )));
    }
    Ok((inputs, outputs))
}

fn parse_side(text: &str, kind: Kind) -> Result<Vec<Field>, SignatureError> {
    text.split(',')
        .map(str::trim)
        .filter(|declaration| !declaration.is_empty())
        .map(|declaration| {
            parse_field(declaration, kind).ok_or_else(|| {
                SignatureError::Definition(format!(
                    "malformed {kind} field {declaration:?}; expected 'name', 'name: type', \
                     or 'name: array[type]'"
                ))
            })
        })
        .collect()
}

fn parse_field(declaration: &str, kind: Kind) -> Option<Field> {
    let (name, ty) = match declaration.split_once(':') {
        Some((name, ty)) => (name.trim(), Some(ty.trim())),
        None => (declaration, None),
    };
    if !is_word(name) {
        return None;
    }
    let mut field = Field::new(name, kind);
    let Some(ty) = ty else {
        return Some(field);
    };

    match ty.split_once('[') {
        Some((outer, rest)) => {
            let element = rest.strip_suffix(']')?.trim();
            let outer = outer.trim();
            if !is_word(outer) || !is_word(element) {
                return None;
            }
            field = field.ty(outer).of(element);
        }
        None if is_word(ty) => field = field.ty(ty),
        None => return None,
    }
    Some(field)
}

fn is_word(text: &str) -> bool {
    !text.is_empty() && text.chars().all(|c| c.is_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(fields: &[Field]) -> Vec<String> {
        fields.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn untyped_fields_default_to_string() {
        let (inputs, outputs) = parse("question -> answer").unwrap();
        assert_eq!(names(&inputs), ["question: string"]);
        assert_eq!(names(&outputs), ["answer: string"]);
    }

    #[test]
    fn several_typed_fields_on_each_side() {
        let (inputs, outputs) =
            parse(" document , style->summary: string,confidence : number ").unwrap();
        assert_eq!(names(&inputs), ["document: string", "style: string"]);
        assert_eq!(names(&outputs), ["summary: string", "confidence: number"]);
    }

    #[test]
    fn array_element_types() {
        let (_, outputs) = parse("text -> tags: array [ string ]").unwrap();
        assert_eq!(names(&outputs), ["tags: array[string]"]);
    }

    #[test]
    fn arrow_count_is_enforced() {
        for bad in ["question answer", "a -> b -> c"] {
            let err = parse(bad).unwrap_err();
            assert!(err.to_string().contains("exactly one '->'"), "{bad}: {err}");
        }
    }

    #[test]
    fn empty_sides_are_rejected() {
        assert!(parse(" -> answer")
            .unwrap_err()
            .to_string()
            .contains("no input fields"));
        assert!(parse("question -> ,")
            .unwrap_err()
            .to_string()
            .contains("no output fields"));
    }

    #[test]
    fn malformed_declarations_are_named() {
        for bad in [
            "a b -> c",
            "a -> b: array[",
            "a -> b: x y",
            "a -> b: array[]",
            "a -> : int",
        ] {
            let err = parse(bad).unwrap_err();
            assert!(err.to_string().contains("malformed"), "{bad}: {err}");
        }
    }
}
