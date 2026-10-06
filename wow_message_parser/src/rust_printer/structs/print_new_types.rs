use crate::parser::types::array::ArraySize;
use crate::parser::types::container::Container;
use crate::parser::types::if_statement::IfStatement;
use crate::parser::types::struct_member::StructMember;
use crate::rust_printer::rust_view::rust_definer::RustDefiner;
use crate::rust_printer::rust_view::rust_type::RustType;
use crate::rust_printer::rust_view::{
    flag_condition_expression, uses_separate_flag_if_else_fields,
};
use crate::rust_printer::structs::print_common_impls::print_size::{
    print_rust_members_sizes, variable_size,
};
use crate::rust_printer::structs::print_derives;
use crate::rust_printer::writer::Writer;
use crate::rust_printer::{get_new_flag_type_name, DefinerType};

pub(crate) fn print_new_types(s: &mut Writer, e: &Container) {
    for rd in e.rust_object().get_rust_definers() {
        match rd.definer_type() {
            DefinerType::Enum => {
                let contains_wrath_spline = rd.all_members().iter().any(|member| {
                    matches!(
                        member.ty(),
                        RustType::MonsterMoveSpline(encoding) if encoding.is_wrath()
                    ) || matches!(member.ty(), RustType::FullMonsterMoveSpline)
                });
                let derive_default = contains_wrath_spline
                    && !rd.is_single_rust_definer()
                    && rd.enumerators().iter().any(|a| !a.has_members_in_struct());
                if !rd.is_single_rust_definer() {
                    print_new_enum_declaration(s, &rd, rd.ty_name(), derive_default);
                }

                if !rd.is_elseif() && !derive_default {
                    print_default_for_new_enum(s, &rd);
                }

                let ty_name = rd.ty_name();
                s.bodyn(format!("impl {ty_name}"), |s| {
                    print_enum_as_int(s, &rd);
                });
                print_enum_display(s, &rd);

                if !rd.is_single_rust_definer() {
                    print_size_for_new_enum(s, &rd);
                }
            }
            DefinerType::Flag => {
                print_new_flag_declaration(s, &rd);

                s.body(format!("impl {name}", name = rd.ty_name()), |s| {
                    print_constructors_for_new_flag(s, &rd, e.tags().rust_strict_conditionals());
                    print_flag_as_int(s, &rd);
                });
                print_size_for_new_flag(s, &rd);

                print_types_for_new_flag(s, &rd);
            }
        }
    }

    if e.tags().rust_strict_conditionals() {
        print_strict_container_api(s, e);
    }
}

fn print_strict_container_api(s: &mut Writer, e: &Container) {
    let members = e.rust_object().members_in_struct().collect::<Vec<_>>();
    let optional = e.rust_object().optional();
    let mut parameters = members
        .iter()
        .map(|member| {
            let ty = if member.is_optional() {
                format!("Option<{}>", member.ty())
            } else {
                member.ty().rust_str()
            };
            format!("{}: {ty}", member.name())
        })
        .collect::<Vec<_>>();
    if let Some(optional) = optional {
        parameters.push(format!("{}: Option<{}>", optional.name(), optional.ty()));
    }

    let mut conditionals = Vec::new();
    collect_strict_if_else_members(e.members(), &mut conditionals);

    s.bodyn(format!("impl {}", e.name()), |s| {
        s.funcn_pub(
            format!("try_new({})", parameters.join(", ")),
            "Result<Self, std::io::Error>",
            |s| {
                for statement in &conditionals {
                    let all_names = statement
                        .all_definitions()
                        .iter()
                        .map(|definition| definition.name().to_string())
                        .collect::<Vec<_>>();
                    let mut valid = branch_presence(statement.else_members(), &all_names);
                    for else_if in statement.else_ifs().iter().rev() {
                        let branch = branch_presence(else_if.members(), &all_names);
                        let condition = flag_condition_expression(
                            else_if.variable_name(),
                            else_if.equation(),
                            "get_",
                        );
                        valid = format!("if {condition} {{ {branch} }} else {{ {valid} }}");
                    }
                    let branch = branch_presence(statement.members(), &all_names);
                    let condition = flag_condition_expression(
                        statement.variable_name(),
                        statement.equation(),
                        "get_",
                    );
                    valid = format!("if {condition} {{ {branch} }} else {{ {valid} }}");
                    s.body(format!("if !({valid})"), |s| {
                        s.wln("return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, \"conditional fields do not match flag condition\"));");
                    });
                }

                s.body_closing_with("Ok(Self", |s| {
                    for member in &members {
                        s.wln(format!("{},", member.name()));
                    }
                    if let Some(optional) = optional {
                        s.wln(format!("{},", optional.name()));
                    }
                }, ")");
            },
        );

        for member in &members {
            let ty = if member.is_optional() {
                format!("Option<{}>", member.ty())
            } else {
                member.ty().rust_str()
            };
            s.funcn_pub_const(
                format!("{}(&self)", member.name()),
                format!("&{ty}"),
                |s| s.wln(format!("&self.{}", member.name())),
            );
        }
        if let Some(optional) = optional {
            s.funcn_pub_const(
                format!("{}(&self)", optional.name()),
                format!("&Option<{}>", optional.ty()),
                |s| s.wln(format!("&self.{}", optional.name())),
            );
        }
    });

    s.bodyn(format!("impl Default for {}", e.name()), |s| {
        s.body("fn default() -> Self", |s| {
            let mut default_else_names = Vec::new();
            for statement in &conditionals {
                default_else_names.extend(statement.else_members().iter().filter_map(|member| {
                    match member {
                        StructMember::Definition(definition) => Some(definition.name().to_string()),
                        StructMember::IfStatement(_) | StructMember::OptionalStatement(_) => None,
                    }
                }));
            }
            s.open_curly("Self");
            for member in &members {
                if member.is_optional() {
                    let value = if default_else_names.contains(&member.name().to_string()) {
                        "Some(Default::default())"
                    } else {
                        "None"
                    };
                    s.wln(format!("{}: {value},", member.name()));
                } else {
                    s.wln(format!("{}: Default::default(),", member.name()));
                }
            }
            if let Some(optional) = optional {
                s.wln(format!("{}: None,", optional.name()));
            }
            s.closing_curly();
        });
    });
}

fn collect_strict_if_else_members<'a>(
    members: &'a [StructMember],
    statements: &mut Vec<&'a IfStatement>,
) {
    for member in members {
        match member {
            StructMember::IfStatement(statement) => {
                if uses_separate_flag_if_else_fields(statement) {
                    statements.push(statement);
                }
                collect_strict_if_else_members(statement.members(), statements);
                for else_if in statement.else_ifs() {
                    collect_strict_if_else_members(else_if.members(), statements);
                }
                collect_strict_if_else_members(statement.else_members(), statements);
            }
            StructMember::OptionalStatement(optional) => {
                collect_strict_if_else_members(optional.members(), statements);
            }
            StructMember::Definition(_) => {}
        }
    }
}

fn branch_presence(members: &[StructMember], all_names: &[String]) -> String {
    let branch_names = members
        .iter()
        .filter_map(|member| match member {
            StructMember::Definition(definition) => Some(definition.name()),
            StructMember::IfStatement(_) | StructMember::OptionalStatement(_) => None,
        })
        .collect::<Vec<_>>();

    all_names
        .iter()
        .map(|name| {
            let method = if branch_names.contains(&name.as_str()) {
                "is_some"
            } else {
                "is_none"
            };
            format!("{name}.{method}()")
        })
        .collect::<Vec<_>>()
        .join(" && ")
}

fn print_flag_as_int(s: &mut Writer, rd: &RustDefiner) {
    s.funcn_const("as_int(&self)", rd.int_ty().rust_str(), |s| {
        s.wln("self.inner");
    });
}

fn print_new_flag_declaration(s: &mut Writer, rd: &RustDefiner) {
    print_derives(s, &rd.all_members(), false, true);
    s.new_flag(rd.ty_name(), rd.int_ty().rust_str(), |s| {
        for enumerator in rd.enumerators() {
            if !enumerator.should_not_be_in_flag_types() {
                s.wln(format!(
                    "{variable_name}: Option<{ty_name}>,",
                    variable_name = enumerator.name().to_lowercase(),
                    ty_name = get_new_flag_type_name(rd.ty_name(), enumerator.rust_name()),
                ));
            }
        }
    });
}

fn print_flag_constructor_fields(s: &mut Writer, rd: &RustDefiner) {
    for enumerator in rd.enumerators() {
        if !enumerator.should_not_be_in_flag_types() {
            s.wln(format!("{},", enumerator.name().to_lowercase()));
        }
    }
}

fn print_constructors_for_new_flag(s: &mut Writer, rd: &RustDefiner, strict_conditionals: bool) {
    use std::fmt::Write;

    let complex_enumerators = rd.complex_flag_enumerators();
    let mut function_name = format!("new(inner: {ty}, ", ty = rd.int_ty().rust_str());

    for enumerator in rd.enumerators() {
        if !enumerator.should_not_be_in_flag_types() {
            write!(
                function_name,
                "{variable_name}: Option<{ty_name}>,",
                variable_name = enumerator.name().to_lowercase(),
                ty_name = get_new_flag_type_name(rd.ty_name(), enumerator.rust_name())
            )
            .unwrap();
        }
    }

    write!(function_name, ")").unwrap();

    if !strict_conditionals || complex_enumerators.is_empty() {
        s.funcn_pub_const(function_name.clone(), "Self", |s| {
            s.body("Self", |s| {
                s.wln("inner,");
                for enumerator in rd.enumerators() {
                    if !enumerator.should_not_be_in_flag_types() {
                        s.wln(format!(
                            "{variable_name}, ",
                            variable_name = enumerator.name().to_lowercase(),
                        ));
                    }
                }
            });
        });
    } else {
        let has_zero_condition = complex_enumerators
            .iter()
            .any(|enumerator| enumerator.value().int() == 0);
        if !has_zero_condition {
            s.wln("/// Synchronizes conditional flag bits with payload presence.");
            s.funcn_pub_const(function_name.clone(), "Self", |s| {
                s.wln("let mut inner = inner;");
                for enumerator in &complex_enumerators {
                    let name = enumerator.name().to_lowercase();
                    s.wln(format!(
                        "inner = if {name}.is_some() {{ inner | {ty}::{flag} }} else {{ inner & !{ty}::{flag} }};",
                        ty = rd.original_ty_name(),
                        flag = enumerator.name(),
                    ));
                }
                s.body("Self", |s| {
                    s.wln("inner,");
                    print_flag_constructor_fields(s, rd);
                });
            });
        }

        let try_function_name = function_name.replacen("new(", "try_new(", 1);
        s.wln("/// Constructs a flag value only when conditional payloads match raw flag bits.");
        s.funcn_pub(try_function_name, "Result<Self, std::io::Error>", |s| {
            s.wln(format!("let flags = {}::new(inner);", rd.original_ty_name()));

            for enumerator in &complex_enumerators {
                let name = enumerator.name().to_lowercase();
                s.body(format!("if flags.is_{name}() != {name}.is_some()"), |s| {
                    s.wln("return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, \"conditional fields do not match flag condition\"));");
                });
            }

            s.body_closing_with("Ok(Self", |s| {
                s.wln("inner,");
                print_flag_constructor_fields(s, rd);
            }, ")");
        });
    }

    s.funcn_pub_const("empty()", "Self", |s| {
        s.body("Self", |s| {
            s.wln("inner: 0,");
            for enumerator in rd.complex_flag_enumerators() {
                s.wln(format!(
                    "{name}: None,",
                    name = enumerator.name().to_lowercase()
                ))
            }
        });
    });

    s.funcn_pub_const("is_empty(&self)", "bool", |s| {
        s.wln("self.inner == 0");
        for enumerator in rd.complex_flag_enumerators() {
            s.wln(format!(
                "&& self.{name}.is_none()",
                name = enumerator.name().to_lowercase()
            ))
        }
    });

    const CLIPPY_MISSING_FN: &str = "#[allow(clippy::missing_const_for_fn)] // false positive";

    for enumerator in rd.enumerators() {
        if enumerator.value().int() == 0 {
            continue;
        }

        if !enumerator.has_members_in_struct() {
            s.funcn_pub_const(
                format!("new_{}()", enumerator.name().to_lowercase()),
                "Self",
                |s| {
                    s.body("Self", |s| {
                        s.wln(format!(
                            "inner: {parent}::{name},",
                            parent = rd.original_ty_name(),
                            name = enumerator.name()
                        ));

                        for inner_enumerator in rd.complex_flag_enumerators() {
                            s.wln(format!(
                                "{name}: None,",
                                name = inner_enumerator.name().to_lowercase()
                            ));
                        }
                    });
                },
            );

            s.wln(CLIPPY_MISSING_FN);
            s.funcn_pub(
                format!("set_{}(mut self)", enumerator.name().to_lowercase()),
                "Self",
                |s| {
                    s.wln(format!(
                        "self.inner |= {ty}::{name};",
                        ty = rd.original_ty_name(),
                        name = enumerator.name()
                    ));

                    s.wln("self");
                },
            );

            s.funcn_pub_const(
                format!("get_{}(&self)", enumerator.name().to_lowercase()),
                "bool",
                |s| {
                    if enumerator.value().int() == 0 {
                        s.wln("// Underlying value is 0");
                        s.wln(format!(
                            "self.inner == {ty}::{name}",
                            ty = rd.original_ty_name(),
                            name = enumerator.name()
                        ));
                    } else {
                        s.wln(format!(
                            "(self.inner & {ty}::{name}) != 0",
                            ty = rd.original_ty_name(),
                            name = enumerator.name()
                        ));
                    }
                },
            );
        } else {
            let new_ty = get_new_flag_type_name(rd.ty_name(), enumerator.rust_name());
            s.funcn_pub_const(
                format!(
                    "new_{lower_name}({lower_name}: {new_ty})",
                    lower_name = enumerator.name().to_lowercase(),
                    new_ty = new_ty,
                ),
                "Self",
                |s| {
                    s.body("Self", |s| {
                        if enumerator.contains_elseif() {
                            s.wln(format!(
                                "inner: {lower_name}.as_int(),",
                                lower_name = enumerator.name().to_lowercase(),
                            ));
                        } else {
                            s.wln(format!(
                                "inner: {parent}::{name},",
                                parent = rd.original_ty_name(),
                                name = enumerator.name()
                            ));
                        }

                        for inner_enumerator in rd.complex_flag_enumerators() {
                            if inner_enumerator.name() == enumerator.name() {
                                s.wln(format!(
                                    "{name}: Some({name}),",
                                    name = inner_enumerator.name().to_lowercase()
                                ));
                            } else {
                                s.wln(format!(
                                    "{name}: None,",
                                    name = inner_enumerator.name().to_lowercase()
                                ));
                            }
                        }
                    });
                },
            );

            s.wln(CLIPPY_MISSING_FN);
            s.funcn_pub(
                format!(
                    "set_{lower_name}(mut self, {lower_name}: {new_ty})",
                    lower_name = enumerator.name().to_lowercase(),
                    new_ty = new_ty,
                ),
                "Self",
                |s| {
                    if enumerator.contains_elseif() {
                        s.wln(format!(
                            "self.inner |= {lower_name}.as_int();",
                            lower_name = enumerator.name().to_lowercase(),
                        ));
                    } else {
                        s.wln(format!(
                            "self.inner |= {ty}::{name};",
                            ty = rd.original_ty_name(),
                            name = enumerator.name()
                        ));
                    }

                    s.wln(format!(
                        "self.{name_lower} = Some({name_lower});",
                        name_lower = enumerator.name().to_lowercase()
                    ));

                    s.wln("self");
                },
            );

            s.funcn_pub_const(
                format!("get_{}(&self)", enumerator.name().to_lowercase()),
                format!("Option<&{new_ty}>"),
                |s| {
                    s.wln(format!(
                        "self.{}.as_ref()",
                        enumerator.name().to_lowercase()
                    ));
                },
            );
        }

        s.wln(CLIPPY_MISSING_FN);
        s.funcn_pub(
            format!("clear_{}(mut self)", enumerator.name().to_lowercase()),
            "Self",
            |s| {
                let clear_flag = if strict_conditionals {
                    format!(
                        "self.inner &= !{ty}::{name};",
                        ty = rd.original_ty_name(),
                        name = enumerator.name()
                    )
                } else {
                    format!(
                        "self.inner &= {ty}::{name}.reverse_bits();",
                        ty = rd.original_ty_name(),
                        name = enumerator.name()
                    )
                };
                s.wln(clear_flag);
                if enumerator.has_members_in_struct() {
                    s.wln(format!("self.{} = None;", enumerator.name().to_lowercase()));
                }
                s.wln("self");
            },
        );
    }
}

fn print_size_for_new_flag(s: &mut Writer, rd: &RustDefiner) {
    variable_size(s, rd.ty_name(), "size", rd.size_is_const_fn(), |s| {
        s.wln(format!("{size} // inner", size = rd.int_ty().size(),));

        for enumerator in rd.enumerators() {
            if enumerator.should_not_be_in_flag_types() {
                continue;
            }

            s.body("+", |s| {
                s.body_else(
                    format!(
                        "if let Some(s) = &self.{name}",
                        name = enumerator.name().to_lowercase()
                    ),
                    |s| {
                        if let Some(size) = enumerator.is_constant() {
                            s.wln(size.to_string());
                        } else {
                            s.wln("s.size()");
                        }
                    },
                    |s| {
                        s.wln("0");
                    },
                );
            });
        }
    });
}

fn print_types_for_new_flag(s: &mut Writer, rd: &RustDefiner) {
    for enumerator in rd.complex_flag_enumerators() {
        if enumerator.contains_elseif() {
            continue;
        }

        let new_type_name = get_new_flag_type_name(rd.ty_name(), enumerator.rust_name());
        print_derives(s, &enumerator.all_members(), false, true);
        s.new_struct(&new_type_name, |s| {
            for m in enumerator.members_in_struct() {
                s.wln(format!(
                    "pub {name}: {ty},",
                    name = m.name(),
                    ty = m.ty().rust_str(),
                ));
            }
        });

        if enumerator.is_constant().is_none() {
            let const_fn = enumerator
                .members_in_struct()
                .iter()
                .all(|a| a.ty().size_is_const_fn());
            variable_size(s, &new_type_name, "size", const_fn, |s| {
                print_rust_members_sizes(s, enumerator.members(), None, "self.");
            });
        }
    }
}

pub(crate) fn print_new_enum_declaration(
    s: &mut Writer,
    rd: &RustDefiner,
    ty_name: &str,
    derive_default: bool,
) {
    print_derives(s, &rd.all_members(), true, true);
    if derive_default {
        s.wln("#[derive(Default)]");
    }
    let default_enumerator = derive_default
        .then(|| {
            rd.enumerators()
                .iter()
                .find(|enumerator| !enumerator.has_members_in_struct())
                .or_else(|| rd.enumerators().first())
                .map(|enumerator| enumerator.rust_name())
        })
        .flatten();
    s.new_enum("pub", ty_name, |s| {
        for enumerator in rd.enumerators() {
            if default_enumerator == Some(enumerator.rust_name()) {
                s.wln("#[default]");
            }
            s.w(enumerator.rust_name());

            if !enumerator.has_members_in_struct() {
                s.wln_no_indent(",");
                continue;
            }

            s.wln_no_indent(" {");
            s.inc_indent();

            for m in enumerator.members_in_struct() {
                s.wln(format!("{name}: {ty},", name = m.name(), ty = m.ty()));
            }
            s.closing_curly_with(",")
        }
    });
}

fn print_default_for_new_enum(s: &mut Writer, rd: &RustDefiner) {
    let ty_name = rd.ty_name();
    s.bodyn(format!("impl Default for {ty_name}"), |s| {
        s.body("fn default() -> Self", |s| {
            s.wln("// First enumerator without any fields");
            let enumerator = if let Some(enumerator) =
                rd.enumerators().iter().find(|a| !a.has_members_in_struct())
            {
                enumerator
            } else {
                rd.enumerators().first().unwrap()
            };

            if enumerator.has_members_in_struct() {
                s.open_curly(format!("Self::{}", enumerator.rust_name()));

                for m in enumerator.members_in_struct() {
                    match m.ty() {
                        RustType::Array { array, .. } => match array.size() {
                            ArraySize::Fixed(v) => {
                                s.wln(format!(
                                    "{name}: [Default::default(); {size}],",
                                    name = m.name(),
                                    size = v
                                ));
                            }
                            _ => s.wln(format!("{name}: Default::default(),", name = m.name())),
                        },
                        _ => s.wln(format!("{name}: Default::default(),", name = m.name())),
                    }
                }

                s.closing_curly();
            } else {
                s.wln(format!("Self::{}", enumerator.rust_name()));
            }
        });
    });
}

pub(crate) fn print_size_for_new_enum_inner(s: &mut Writer, re: &RustDefiner) {
    s.body("match self", |s| {
        for enumerator in re.enumerators() {
            if !enumerator.has_members() {
                continue;
            }

            let name = enumerator.rust_name();
            if enumerator.has_members_in_struct() {
                s.open_curly(format!("Self::{name}"));

                for m in enumerator.members_in_struct() {
                    if m.ty().size_requires_variable() {
                        s.wln(format!("{},", m.name()));
                    }
                }

                if enumerator
                    .members_in_struct()
                    .iter()
                    .any(|a| !a.ty().size_requires_variable())
                {
                    s.wln("..");
                }

                s.closing_curly_with(" => {");
                s.inc_indent();
            } else {
                s.open_curly(format!("Self::{name} =>"));
            }

            if re.is_elseif() {
                s.wln("// Not an actual enum sent over the wire");
            } else {
                s.wln(format!("{}", re.int_ty().size()));
            }

            print_rust_members_sizes(s, enumerator.members(), Some(re.is_elseif()), "");
            s.closing_curly();
        }

        if re.enumerators().iter().any(|a| !a.has_members()) {
            s.wln(format!("_ => {},", re.int_ty().size()));
        }
    });
}

fn print_size_for_new_enum(s: &mut Writer, re: &RustDefiner) {
    variable_size(s, re.ty_name(), "size", re.size_is_const_fn(), |s| {
        print_size_for_new_enum_inner(s, re)
    });
}

fn print_enum_as_int(s: &mut Writer, rd: &RustDefiner) {
    s.funcn_const("as_int(&self)", rd.int_ty().rust_str(), |s| {
        s.body("match self", |s| {
            for enumerator in rd.enumerators() {
                s.wln(format!(
                    "Self::{enumerator}{extras} => {value},",
                    enumerator = enumerator.rust_name(),
                    value = enumerator.value().int(),
                    extras = match enumerator.has_members_in_struct() {
                        true => " { .. }",
                        false => "",
                    },
                ));
            }
        });
    });
}

fn print_enum_display(s: &mut Writer, rd: &RustDefiner) {
    s.impl_for("std::fmt::Display", rd.ty_name(), |s| {
        s.body(
            "fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result",
            |s| {
                s.body("match self", |s| {
                    for field in rd.enumerators() {
                        let display = field.rust_name();
                        let extra = if field.has_members_in_struct() {
                            "{ .. }"
                        } else {
                            ""
                        };

                        s.wln(format!(
                            r#"Self::{name}{extra} => f.write_str("{display}"),"#,
                            name = field.rust_name(),
                            display = display,
                        ));
                    }
                });
            },
        );
    });
}
