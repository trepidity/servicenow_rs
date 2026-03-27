/// A single filter condition in a ServiceNow encoded query.
#[derive(Debug, Clone)]
pub struct Filter {
    pub field: String,
    pub operator: Operator,
    pub value: String,
}

/// Logical joiner between filter conditions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Joiner {
    And,
    Or,
    NewQuery,
}

/// Query operators supported by ServiceNow's encoded query syntax.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operator {
    Equals,
    NotEquals,
    /// Contains / LIKE — fuzzy substring match.
    Contains,
    /// Not contains / NOT LIKE.
    NotContains,
    StartsWith,
    EndsWith,
    GreaterThan,
    GreaterThanOrEqual,
    LessThan,
    LessThanOrEqual,
    In,
    NotIn,
    IsEmpty,
    IsNotEmpty,
    Between,
    InstanceOf,
}

impl Operator {
    /// Convert to the ServiceNow encoded query operator string.
    pub fn as_encoded(&self) -> &str {
        match self {
            Operator::Equals => "=",
            Operator::NotEquals => "!=",
            Operator::Contains => "LIKE",
            Operator::NotContains => "NOT LIKE",
            Operator::StartsWith => "STARTSWITH",
            Operator::EndsWith => "ENDSWITH",
            Operator::GreaterThan => ">",
            Operator::GreaterThanOrEqual => ">=",
            Operator::LessThan => "<",
            Operator::LessThanOrEqual => "<=",
            Operator::In => "IN",
            Operator::NotIn => "NOT IN",
            Operator::IsEmpty => "ISEMPTY",
            Operator::IsNotEmpty => "ISNOTEMPTY",
            Operator::Between => "BETWEEN",
            Operator::InstanceOf => "INSTANCEOF",
        }
    }
}

/// Sort direction for order-by clauses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Order {
    Asc,
    Desc,
}

/// A query condition with its joiner.
#[derive(Debug, Clone)]
pub struct Condition {
    pub joiner: Joiner,
    pub filter: Filter,
}

/// Validate that a field name contains only safe characters.
///
/// Allows alphanumeric, underscores, hyphens, and dots (for dot-walking).
/// Rejects characters that could break the encoded query syntax (^, =, etc.).
fn validate_field_name(field: &str) -> bool {
    !field.is_empty()
        && field
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-' || c == '.')
}

/// Builds a ServiceNow encoded query string from a list of conditions and ordering.
///
/// Returns an error if any field name contains characters that could break
/// the encoded query syntax.
pub fn encode_query(
    conditions: &[Condition],
    order_by: &[(String, Order)],
) -> crate::error::Result<String> {
    let mut parts = Vec::new();

    for (i, cond) in conditions.iter().enumerate() {
        if i > 0 {
            match cond.joiner {
                Joiner::And => parts.push("^".to_string()),
                Joiner::Or => parts.push("^OR".to_string()),
                Joiner::NewQuery => parts.push("^NQ".to_string()),
            }
        }

        let f = &cond.filter;
        if !validate_field_name(&f.field) {
            return Err(crate::error::Error::Query(format!(
                "invalid field name: '{}' — only alphanumeric, underscore, hyphen, and dot allowed",
                f.field
            )));
        }
        match f.operator {
            Operator::IsEmpty | Operator::IsNotEmpty => {
                parts.push(format!("{}{}", f.field, f.operator.as_encoded()));
            }
            _ => {
                // Escape ^ in values to prevent query injection.
                let escaped_value = f.value.replace('^', "\\^");
                parts.push(format!(
                    "{}{}{}",
                    f.field,
                    f.operator.as_encoded(),
                    escaped_value
                ));
            }
        }
    }

    for (field, order) in order_by {
        if !validate_field_name(field) {
            return Err(crate::error::Error::Query(format!(
                "invalid order-by field name: '{}' — only alphanumeric, underscore, hyphen, and dot allowed",
                field
            )));
        }
        match order {
            Order::Asc => parts.push(format!("^ORDERBY{}", field)),
            Order::Desc => parts.push(format!("^ORDERBYDESC{}", field)),
        }
    }

    Ok(parts.join(""))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_query() {
        let conditions = vec![Condition {
            joiner: Joiner::And,
            filter: Filter {
                field: "state".to_string(),
                operator: Operator::Equals,
                value: "1".to_string(),
            },
        }];
        assert_eq!(encode_query(&conditions, &[]).unwrap(), "state=1");
    }

    #[test]
    fn test_compound_query() {
        let conditions = vec![
            Condition {
                joiner: Joiner::And,
                filter: Filter {
                    field: "state".to_string(),
                    operator: Operator::Equals,
                    value: "1".to_string(),
                },
            },
            Condition {
                joiner: Joiner::And,
                filter: Filter {
                    field: "priority".to_string(),
                    operator: Operator::LessThan,
                    value: "3".to_string(),
                },
            },
        ];
        assert_eq!(
            encode_query(&conditions, &[]).unwrap(),
            "state=1^priority<3"
        );
    }

    #[test]
    fn test_or_query() {
        let conditions = vec![
            Condition {
                joiner: Joiner::And,
                filter: Filter {
                    field: "state".to_string(),
                    operator: Operator::Equals,
                    value: "1".to_string(),
                },
            },
            Condition {
                joiner: Joiner::Or,
                filter: Filter {
                    field: "state".to_string(),
                    operator: Operator::Equals,
                    value: "2".to_string(),
                },
            },
        ];
        assert_eq!(encode_query(&conditions, &[]).unwrap(), "state=1^ORstate=2");
    }

    #[test]
    fn test_query_with_ordering() {
        let conditions = vec![Condition {
            joiner: Joiner::And,
            filter: Filter {
                field: "active".to_string(),
                operator: Operator::Equals,
                value: "true".to_string(),
            },
        }];
        let order = vec![("sys_created_on".to_string(), Order::Desc)];
        assert_eq!(
            encode_query(&conditions, &order).unwrap(),
            "active=true^ORDERBYDESCsys_created_on"
        );
    }

    #[test]
    fn test_is_empty_operator() {
        let conditions = vec![Condition {
            joiner: Joiner::And,
            filter: Filter {
                field: "assigned_to".to_string(),
                operator: Operator::IsEmpty,
                value: String::new(),
            },
        }];
        assert_eq!(
            encode_query(&conditions, &[]).unwrap(),
            "assigned_toISEMPTY"
        );
    }

    #[test]
    fn test_contains_query() {
        let conditions = vec![Condition {
            joiner: Joiner::And,
            filter: Filter {
                field: "short_description".to_string(),
                operator: Operator::Contains,
                value: "network".to_string(),
            },
        }];
        assert_eq!(
            encode_query(&conditions, &[]).unwrap(),
            "short_descriptionLIKEnetwork"
        );
    }
}
