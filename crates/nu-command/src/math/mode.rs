use crate::math::utils::run_with_function;
use nu_protocol::ast::Call;
use nu_protocol::engine::{Command, EngineState, Stack};
use nu_protocol::{
    record, Category, Example, PipelineData, ShellError, Signature, Span, Type, Value,
};
use std::cmp::Ordering;
use std::collections::HashMap;

#[derive(Clone)]
pub struct SubCommand;

#[derive(Hash, Eq, PartialEq, Debug)]
enum NumberTypes {
    Float,
    Int,
    Duration,
    Filesize,
}

#[derive(Hash, Eq, PartialEq, Debug)]
struct HashableType {
    bytes: [u8; 8],
    original_type: NumberTypes,
}

impl HashableType {
    fn new(bytes: [u8; 8], original_type: NumberTypes) -> HashableType {
        HashableType {
            bytes,
            original_type,
        }
    }
}

impl Command for SubCommand {
    fn name(&self) -> &str {
        "math mode"
    }

    fn signature(&self) -> Signature {
        Signature::build("math mode")
            .input_output_types(vec![
                (
                    Type::List(Box::new(Type::Number)),
                    Type::List(Box::new(Type::Number)),
                ),
                (
                    Type::List(Box::new(Type::Duration)),
                    Type::List(Box::new(Type::Duration)),
                ),
                (
                    Type::List(Box::new(Type::Filesize)),
                    Type::List(Box::new(Type::Filesize)),
                ),
                (Type::Table(vec![]), Type::Record(vec![])),
            ])
            .allow_variants_without_examples(true)
            .category(Category::Math)
    }

    fn usage(&self) -> &str {
        "Returns the most frequent element(s) from a list of numbers or tables."
    }

    fn search_terms(&self) -> Vec<&str> {
        vec!["common", "often"]
    }

    fn run(
        &self,
        _engine_state: &EngineState,
        _stack: &mut Stack,
        call: &Call,
        input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        run_with_function(call, input, mode)
    }

    fn examples(&self) -> Vec<Example> {
        vec![
            Example {
                description: "Compute the mode(s) of a list of numbers",
                example: "[3 3 9 12 12 15] | math mode",
                result: Some(Value::test_list(vec![
                    Value::test_int(3),
                    Value::test_int(12),
                ])),
            },
            Example {
                description: "Compute the mode(s) of the columns of a table",
                example: "[{a: 1 b: 3} {a: 2 b: -1} {a: 1 b: 5}] | math mode",
                result: Some(Value::test_record(record! {
                        "a" => Value::list(vec![Value::test_int(1)], Span::test_data()),
                        "b" => Value::list(
                            vec![Value::test_int(-1), Value::test_int(3), Value::test_int(5)],
                            Span::test_data(),
                        ),
                })),
            },
        ]
    }
}

pub fn mode(values: &[Value], _span: Span, head: Span) -> Result<Value, ShellError> {
    if let Some(Err(values)) = values
        .windows(2)
        .map(|elem| {
            if elem[0].partial_cmp(&elem[1]).is_none() {
                return Err(ShellError::OperatorMismatch {
                    op_span: head,
                    lhs_ty: elem[0].get_type().to_string(),
                    lhs_span: elem[0].span(),
                    rhs_ty: elem[1].get_type().to_string(),
                    rhs_span: elem[1].span(),
                });
            }
            Ok(elem[0].partial_cmp(&elem[1]).unwrap_or(Ordering::Equal))
        })
        .find(|elem| elem.is_err())
    {
        return Err(values);
    }
    //In e-q, Value doesn't implement Hash or Eq, so we have to get the values inside
    // But f64 doesn't implement Hash, so we get the binary representation to use as
    // key in the HashMap
    let hashable_values = values
        .iter()
        .map(|val| match val {
            Value::Int { val, .. } => Ok(HashableType::new(val.to_ne_bytes(), NumberTypes::Int)),
            Value::Duration { val, .. } => {
                Ok(HashableType::new(val.to_ne_bytes(), NumberTypes::Duration))
            }
            Value::Float { val, .. } => {
                Ok(HashableType::new(val.to_ne_bytes(), NumberTypes::Float))
            }
            Value::Filesize { val, .. } => {
                Ok(HashableType::new(val.to_ne_bytes(), NumberTypes::Filesize))
            }
            Value::Error { error, .. } => Err(*error.clone()),
            other => Err(ShellError::UnsupportedInput(
                "Unable to give a result with this input".to_string(),
                "value originates from here".into(),
                head,
                other.span(),
            )),
        })
        .collect::<Result<Vec<HashableType>, ShellError>>()?;

    let mut frequency_map = HashMap::new();
    for v in hashable_values {
        let counter = frequency_map.entry(v).or_insert(0);
        *counter += 1;
    }

    let mut max_freq = -1;
    let mut modes = Vec::<Value>::new();
    for (value, frequency) in &frequency_map {
        match max_freq.cmp(frequency) {
            Ordering::Less => {
                max_freq = *frequency;
                modes.clear();
                modes.push(recreate_value(value, head));
            }
            Ordering::Equal => {
                modes.push(recreate_value(value, head));
            }
            Ordering::Greater => (),
        }
    }

    modes.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
    Ok(Value::list(modes, head))
}

fn recreate_value(hashable_value: &HashableType, head: Span) -> Value {
    let bytes = hashable_value.bytes;
    match &hashable_value.original_type {
        NumberTypes::Int => Value::int(i64::from_ne_bytes(bytes), head),
        NumberTypes::Float => Value::float(f64::from_ne_bytes(bytes), head),
        NumberTypes::Duration => Value::duration(i64::from_ne_bytes(bytes), head),
        NumberTypes::Filesize => Value::filesize(i64::from_ne_bytes(bytes), head),
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_examples() {
        use crate::test_examples;

        test_examples(SubCommand {})
    }
}
