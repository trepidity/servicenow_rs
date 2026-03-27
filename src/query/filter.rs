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
    Contains,
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
    Like,
    NotLike,
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
            Operator::Like => "LIKE",
            Operator::NotLike => "NOT LIKE",
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

/// Builds a ServiceNow encoded query string from a list of conditions and ordering.
pub fn encode_query(conditions: &[Condition], order_by: &[(String, Order)]) -> String {
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
        match f.operator {
            Operator::IsEmpty | Operator::IsNotEmpty => {
                parts.push(format!("{}{}", f.field, f.operator.as_encoded()));
            }
            _ => {
                parts.push(format!(
                    "{}{}{}",
                    f.field,
                    f.operator.as_encoded(),
                    f.value
                ));
            }
        }
    }

    for (field, order) in order_by {
        match order {
            Order::Asc => parts.push(format!("^ORDERBY{}", field)),
            Order::Desc => parts.push(format!("^ORDERBYDESC{}", field)),
        }
    }

    parts.join("")
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
        assert_eq!(encode_query(&conditions, &[]), "state=1");
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
        assert_eq!(encode_query(&conditions, &[]), "state=1^priority<3");
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
        assert_eq!(encode_query(&conditions, &[]), "state=1^ORstate=2");
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
            encode_query(&conditions, &order),
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
        assert_eq!(encode_query(&conditions, &[]), "assigned_toISEMPTY");
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
            encode_query(&conditions, &[]),
            "short_descriptionLIKEnetwork"
        );
    }
}
