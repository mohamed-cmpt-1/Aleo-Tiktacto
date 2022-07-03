// Copyright (C) 2019-2022 Aleo Systems Inc.
// This file is part of the Leo library.

// The Leo library is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// The Leo library is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with the Leo library. If not, see <https://www.gnu.org/licenses/>.

use crate::{Declaration, TypeChecker, VariableSymbol};
use leo_ast::*;
use leo_errors::TypeCheckerError;

use leo_span::sym;
use std::collections::HashSet;

impl<'a> ProgramVisitor<'a> for TypeChecker<'a> {
    fn visit_function(&mut self, input: &'a Function) {
        self.has_return = false;
        self.symbol_table.clear_variables();
        self.parent = Some(input.name());
        input.input.iter().for_each(|i| {
            let input_var = i.get_variable();
            self.check_ident_type(&Some(input_var.type_));

            // Check for conflicting variable names.
            if let Err(err) = self.symbol_table.insert_variable(
                input_var.identifier.name,
                VariableSymbol {
                    type_: &input_var.type_,
                    span: input_var.identifier.span(),
                    declaration: Declaration::Input(input_var.mode()),
                },
            ) {
                self.handler.emit_err(err);
            }
        });
        self.visit_block(&input.block);

        if !self.has_return {
            self.handler
                .emit_err(TypeCheckerError::function_has_no_return(input.name(), input.span()).into());
        }
    }

    fn visit_circuit(&mut self, input: &'a Circuit) {
        // Check for conflicting circuit member names.
        let mut used = HashSet::new();
        if !input.members.iter().all(|member| used.insert(member.name())) {
            self.handler.emit_err(if input.is_record {
                TypeCheckerError::duplicate_record_variable(input.name(), input.span()).into()
            } else {
                TypeCheckerError::duplicate_circuit_member(input.name(), input.span()).into()
            });
        }

        // For records, enforce presence of `owner: Address` and `balance: u64` members.
        if input.is_record {
            let check_has_field = |need, expected_ty: Type| match input
                .members
                .iter()
                .find_map(|CircuitMember::CircuitVariable(v, t)| (v.name == need).then(|| (v, t)))
            {
                Some((_, actual_ty)) if expected_ty.eq_flat(actual_ty) => {} // All good, found + right type!
                Some((field, _)) => {
                    self.handler
                        .emit_err(TypeCheckerError::record_var_wrong_type(field, expected_ty, input.span()).into());
                }
                None => {
                    self.handler
                        .emit_err(TypeCheckerError::required_record_variable(need, expected_ty, input.span()).into());
                }
            };
            check_has_field(sym::owner, Type::Address);
            check_has_field(sym::balance, Type::IntegerType(IntegerType::U64));
        }
    }
}
