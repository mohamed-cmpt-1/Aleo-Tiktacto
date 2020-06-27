use crate::{
    constraints::{ConstrainedProgram, ConstrainedValue},
    errors::constraints::ImportError,
    new_scope,
    GroupType,
};
use leo_ast::LeoParser;
use leo_types::{Import, ImportSymbol, Package, PackageAccess, Program, Span};

use snarkos_models::curves::{Field, PrimeField};
use std::{env::current_dir, fs, fs::DirEntry, path::PathBuf};

static SOURCE_FILE_EXTENSION: &str = ".leo";
static SOURCE_DIRECTORY_NAME: &str = "src/";
// pub(crate) static IMPORTS_DIRECTORY_NAME: &str = "imports/";

fn parse_import_file(entry: &DirEntry, span: &Span) -> Result<Program, ImportError> {
    // make sure the given entry is file
    let file_type = entry
        .file_type()
        .map_err(|error| ImportError::directory_error(error, span.clone()))?;
    let file_name = entry
        .file_name()
        .into_string()
        .map_err(|_| ImportError::convert_os_string(span.clone()))?;

    if file_type.is_dir() {
        return Err(ImportError::expected_file(file_name, span.clone()));
    }

    // Build the abstract syntax tree
    let file_path = &entry.path();
    let input_file = &LeoParser::load_file(file_path)?;
    let syntax_tree = LeoParser::parse_file(file_path, input_file)?;

    // Generate aleo program from file
    Ok(Program::from(syntax_tree, file_name.clone()))
}

impl<F: Field + PrimeField, G: GroupType<F>> ConstrainedProgram<F, G> {
    pub fn enforce_import_star(&mut self, scope: String, entry: &DirEntry, span: Span) -> Result<(), ImportError> {
        let mut program = parse_import_file(entry, &span)?;

        // Use same namespace as calling function for imported symbols
        program = program.name(scope);

        // * -> import all imports, circuits, functions in the current scope
        self.resolve_definitions(program)
    }

    pub fn enforce_import_symbol(
        &mut self,
        scope: String,
        entry: &DirEntry,
        symbol: ImportSymbol,
    ) -> Result<(), ImportError> {
        // Generate aleo program from file
        let mut program = parse_import_file(entry, &symbol.span)?;

        // Use same namespace as calling function for imported symbols
        program = program.name(scope);

        let program_name = program.name.clone();

        // match each import symbol to a symbol in the imported file
        // for symbol in import.symbols.into_iter() {
        // see if the imported symbol is a circuit
        let matched_circuit = program
            .circuits
            .clone()
            .into_iter()
            .find(|(circuit_name, _circuit_def)| symbol.symbol == *circuit_name);

        let value = match matched_circuit {
            Some((_circuit_name, circuit_def)) => ConstrainedValue::CircuitDefinition(circuit_def),
            None => {
                // see if the imported symbol is a function
                let matched_function = program
                    .functions
                    .clone()
                    .into_iter()
                    .find(|(function_name, _function)| symbol.symbol == *function_name);

                match matched_function {
                    Some((_function_name, function)) => ConstrainedValue::Function(None, function),
                    None => return Err(ImportError::unknown_symbol(symbol, program_name, &entry.path())),
                }
            }
        };

        // take the alias if it is present
        let name = symbol.alias.unwrap_or(symbol.symbol);
        let resolved_name = new_scope(program_name.clone(), name.to_string());

        // store imported circuit under resolved name
        self.store(resolved_name, value);
        // }

        // evaluate all import statements in imported file
        // todo: add logic to detect import loops
        program
            .imports
            .into_iter()
            .map(|nested_import| self.enforce_import(program_name.clone(), nested_import))
            .collect::<Result<Vec<_>, ImportError>>()?;

        Ok(())
    }

    pub fn enforce_package_access(
        &mut self,
        scope: String,
        entry: &DirEntry,
        access: PackageAccess,
    ) -> Result<(), ImportError> {
        // bring one or more import symbols into scope for the current constrained program
        // we will recursively traverse sub packages here until we find the desired symbol
        match access {
            PackageAccess::Star(span) => self.enforce_import_star(scope, entry, span),
            PackageAccess::Symbol(symbol) => self.enforce_import_symbol(scope, entry, symbol),
            PackageAccess::SubPackage(package) => self.enforce_package(scope, entry.path(), *package),
            PackageAccess::Multiple(accesses) => {
                for access in accesses {
                    self.enforce_package_access(scope.clone(), entry, access)?;
                }

                Ok(())
            }
        }
    }

    pub fn enforce_package(&mut self, scope: String, path: PathBuf, package: Package) -> Result<(), ImportError> {
        let package_name = package.name;

        // search for package name in local src directory
        let mut source_directory = path.clone();
        source_directory.push(SOURCE_DIRECTORY_NAME);

        let entries = fs::read_dir(source_directory)
            .map_err(|error| ImportError::directory_error(error, package_name.span.clone()))?
            .into_iter()
            .collect::<Result<Vec<_>, std::io::Error>>()
            .map_err(|error| ImportError::directory_error(error, package_name.span.clone()))?;

        let matched_source_entry = entries.into_iter().find(|entry| {
            entry
                .file_name()
                .into_string()
                .unwrap()
                .trim_end_matches(SOURCE_FILE_EXTENSION)
                .eq(&package_name.name)
        });

        // search for package name in imports directory
        // let mut source_directory = path.clone();
        // source_directory.push(IMPORTS_DIRECTORY_NAME);
        //
        // let entries = fs::read_dir(source_directory)
        //     .map_err(|error| ImportError::directory_error(error, package_name.span.clone()))?
        //     .into_iter()
        //     .collect::<Result<Vec<_>, std::io::Error>>()
        //     .map_err(|error| ImportError::directory_error(error, package_name.span.clone()))?;
        //
        // let matched_import_entry = entries.into_iter().find(|entry| {
        //     entry.file_name().eq(&package_name.name)
        // });

        // todo: return error if package name is present in both directories

        // Enforce package access
        if let Some(entry) = matched_source_entry {
            self.enforce_package_access(scope, &entry, package.access)
        } else {
            Err(ImportError::unknown_package(package_name))
        }
    }

    pub fn enforce_import(&mut self, scope: String, import: Import) -> Result<(), ImportError> {
        let path = current_dir().map_err(|error| ImportError::directory_error(error, import.span.clone()))?;

        self.enforce_package(scope, path, import.package)
    }
}
