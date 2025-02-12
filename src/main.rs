use std::{error::Error, fs, path};
use serde::{Deserialize, Serialize};
use derivative::Derivative;

fn main() {
    dbg!(get_join_tree("profile.json").unwrap());
}

#[derive(Serialize, Deserialize, Debug, Hash, PartialEq, Eq)]
pub enum JoinType {
    Inner,
    LeftOuter,
    RightOuter,
    FullOuter,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Hash)]
pub struct Attribute {
    pub table_name: String,
    pub attr_name: String,
}

impl std::fmt::Debug for Attribute {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.table_name, self.attr_name)
    }
}

#[derive(Serialize, Deserialize, Debug, Hash, PartialEq, Eq)]
pub struct Condition {
    pub left_attr: Attribute,
    pub right_attr: Attribute,
}

#[derive(Serialize, Deserialize, Debug, Hash, PartialEq, Eq)]
pub struct Join {
    pub join_type: JoinType,
    pub equalizers: Vec<Condition>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Hash, PartialEq, Eq)]
pub struct Scan {
    pub table_name: String,
    pub attributes: Vec<Attribute>,
}

#[derive(Serialize, Deserialize, Debug, Hash, PartialEq, Eq)]
pub struct Project {
    columns: Vec<Attribute>,
}

#[derive(Serialize, Deserialize, Debug, Hash, PartialEq, Eq)]
pub enum Op {
    Join(Join),
    Scan(Scan),
    Project(Project),
    Filter,
}

#[derive(Derivative)]
#[derivative(Debug, Hash, PartialEq, Eq)]
#[derive(Serialize, Deserialize)]
pub struct Node {
    pub name: String,
    #[derivative(PartialEq = "ignore")]
    #[derivative(Hash = "ignore")]
    pub timing: f64,
    pub cardinality: u64,
    pub extra_info: String,
    pub children: Vec<Box<Node>>,
    pub attr: Option<Op>,
}

pub fn get_join_tree(file_name: &str) -> Result<Node, Box<dyn Error>> {
    let sql = fs::read_to_string(path::Path::new(file_name))?;
    let mut root: Node = serde_json::from_str(sql.as_str())?;
    parse_tree_extra_info(&mut root);
    Ok(root)
}

pub fn parse_tree_extra_info(root: &mut Node) {
    let mut parse_func = |node: &mut Node| match node.name.as_str() {
        "HASH_JOIN" => {
            let extra_info: Vec<_> = node
                .extra_info
                .split('\n')
                .filter(|s| !s.is_empty())
                .collect();

            let join_type = match extra_info[0] {
                "INNER" => JoinType::Inner,
                "MARK" => return,
                _ => panic!("Fail to parse Join Type {}", extra_info[0]),
            };

            let mut equalizers = Vec::new();

            for pred in &extra_info[1..] {
                let equalizer = pred.split('=').map(|s| s.trim()).collect::<Vec<_>>();
                let left_attr = equalizer[0]
                    .split('.')
                    .map(|s| s.trim())
                    .collect::<Vec<_>>();
                let right_attr = equalizer[1]
                    .split('.')
                    .map(|s| s.trim())
                    .collect::<Vec<_>>();
                // HACK in the profile generated by unmodified duckdb
                // the table name is not included in the attribute name.
                // Here we use the attribute name as deadbeef,
                // and get the table name from the profile generated
                // by patched duckdb
                equalizers.push(Condition {
                    left_attr: if left_attr.len() == 1 {
                        Attribute {
                            table_name: left_attr[0].to_string(),
                            attr_name: left_attr[0].to_string(),
                        }
                    } else {
                        Attribute {
                            table_name: left_attr[0].to_string(),
                            attr_name: left_attr[1].to_string(),
                        }
                    },
                    right_attr: if right_attr.len() == 1 {
                        Attribute {
                            table_name: right_attr[0].to_string(),
                            attr_name: right_attr[0].to_string(),
                        }
                    } else {
                        Attribute {
                            table_name: right_attr[0].to_string(),
                            attr_name: right_attr[1].to_string(),
                        }
                    },
                });
            }

            node.attr = Some(Op::Join(Join {
                join_type,
                equalizers,
            }));
        }
        "SEQ_SCAN" => {
            let extra_info: Vec<_> = node.extra_info.split("[INFOSEPARATOR]").collect();
            let table_name = extra_info[0].trim();
            let info_strs: Vec<_> = extra_info[1]
                .split('\n')
                .filter(|s| !s.is_empty())
                .collect();

            node.attr = Some(Op::Scan(Scan {
                table_name: table_name.to_string(),
                attributes: info_strs
                    .iter()
                    .map(|s| Attribute {
                        table_name: table_name.to_string(),
                        attr_name: s.to_string(),
                    })
                    .collect(),
            }));
        }
        "PROJECTION" => {
            let columns: Vec<_> = node
                .extra_info
                .split('\n')
                .filter(|s| !s.is_empty())
                .map(|s| {
                    let names: Vec<_> = s.split('.').map(|s| s.trim()).collect();
                    // HACK similar to the above, we use "" as deadbeef
                    // and get the table name from profile by the patched duckdb.
                    if names.len() == 1 {
                        Attribute {
                            table_name: "".to_string(),
                            attr_name: names[0].to_string(),
                        }
                    } else {
                        Attribute {
                            table_name: names[0].to_string(),
                            attr_name: names[1].to_string(),
                        }
                    }
                })
                .collect();
            node.attr = Some(Op::Project(Project { columns }));
        }
        "CHUNK_SCAN" | "RESULT_COLLECTOR" | "SIMPLE_AGGREGATE" | "Query" => {}
        "FILTER" => {
            node.attr = Some(Op::Filter);
        }
        _ => panic!("Unknown node type {}", node.name),
    };
    inorder_traverse_mut(root, &mut parse_func);

}

fn inorder_traverse_mut<T>(node: &mut Node, func: &mut T)
where
    T: FnMut(&mut Node),
{
    if !node.children.is_empty() {
        inorder_traverse_mut(&mut node.children[0], func);
    }
    func(node);
    if !node.children.is_empty() {
        for child_node in &mut node.children[1..] {
            inorder_traverse_mut(child_node, func);
        }
    }
}