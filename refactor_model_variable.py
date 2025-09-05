import os

def refactor_rust_files(root_dir):
    """
    Recursively finds all .rs files in a directory and replaces all
    occurrences of 'CONFIG.model' with 'CONFIG.gpt5_model'.
    """
    print("Starting refactoring process...")
    files_modified = 0
    for subdir, _, files in os.walk(root_dir):
        for file in files:
            if file.endswith(".rs"):
                file_path = os.path.join(subdir, file)
                try:
                    with open(file_path, 'r', encoding='utf-8') as f:
                        content = f.read()

                    if 'CONFIG.model' in content:
                        print(f"Updating file: {file_path}")
                        new_content = content.replace('CONFIG.model', 'CONFIG.gpt5_model')
                        
                        with open(file_path, 'w', encoding='utf-8') as f:
                            f.write(new_content)
                        files_modified += 1

                except Exception as e:
                    print(f"Error processing file {file_path}: {e}")

    print(f"\nRefactoring complete. {files_modified} files were modified.")

if __name__ == "__main__":
    # Assuming the script is run from the root of the 'backend' directory
    source_directory = "src"
    if not os.path.isdir(source_directory):
        print(f"Error: The '{source_directory}' directory was not found.")
        print("Please run this script from the root of your 'mira/backend' project.")
    else:
        refactor_rust_files(source_directory)
