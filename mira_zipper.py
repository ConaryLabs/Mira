#!/usr/bin/env python3
"""
Mira Backend Directory Zipper
Creates a shareable zip of the Mira backend project
"""

import os
import zipfile
from datetime import datetime
from pathlib import Path
import argparse

def should_include(path, excludes):
    """Check if a file/directory should be included in the zip"""
    path_str = str(path)
    
    # Default excludes
    default_excludes = {
        'target',           # Rust build artifacts
        '.git',            # Git repository
        'node_modules',    # Node dependencies
        '.env',            # Environment variables
        '*.db',            # SQLite databases
        '__pycache__',     # Python cache
        '.DS_Store',       # macOS files
        '*.swp',           # Vim swap files
        'qdrant_data',     # Qdrant data directory
        'mira.db*',        # Database files
    }
    
    # Check against excludes
    for exclude in default_excludes.union(excludes):
        if exclude.startswith('*'):
            # Handle wildcard patterns
            if path_str.endswith(exclude[1:]):
                return False
        elif exclude in path.parts or path_str.endswith(exclude):
            return False
    
    return True

def zip_directory(source_dir, output_file, excludes=None):
    """Create a zip file of the directory"""
    if excludes is None:
        excludes = set()
    
    source_path = Path(source_dir)
    if not source_path.exists():
        raise FileNotFoundError(f"Source directory not found: {source_dir}")
    
    included_files = []
    excluded_files = []
    
    with zipfile.ZipFile(output_file, 'w', zipfile.ZIP_DEFLATED) as zipf:
        for root, dirs, files in os.walk(source_path):
            root_path = Path(root)
            
            # Filter directories to prevent walking into excluded ones
            dirs[:] = [d for d in dirs if should_include(root_path / d, excludes)]
            
            for file in files:
                file_path = root_path / file
                if should_include(file_path, excludes):
                    # Calculate relative path for the zip
                    arcname = file_path.relative_to(source_path.parent)
                    zipf.write(file_path, arcname)
                    included_files.append(str(arcname))
                else:
                    excluded_files.append(str(file_path.relative_to(source_path)))
    
    return included_files, excluded_files

def main():
    parser = argparse.ArgumentParser(description='Zip up the Mira backend directory')
    parser.add_argument(
        '--source', '-s',
        default='/home/peter/mira/backend',
        help='Source directory to zip (default: /home/peter/mira/backend)'
    )
    parser.add_argument(
        '--output', '-o',
        default=None,
        help='Output zip file name (default: mira-backend-YYYYMMDD-HHMMSS.zip)'
    )
    parser.add_argument(
        '--exclude', '-e',
        action='append',
        default=[],
        help='Additional patterns to exclude (can be used multiple times)'
    )
    parser.add_argument(
        '--list', '-l',
        action='store_true',
        help='List files that will be included/excluded without creating zip'
    )
    
    args = parser.parse_args()
    
    # Generate output filename if not provided
    if args.output is None:
        timestamp = datetime.now().strftime('%Y%m%d-%H%M%S')
        args.output = f'mira-backend-{timestamp}.zip'
    
    # Convert exclude list to set
    excludes = set(args.exclude)
    
    print(f"Preparing to zip: {args.source}")
    print(f"Output file: {args.output}")
    
    if args.list:
        # Dry run - just list what would be included/excluded
        print("\nAnalyzing directory structure...")
        included = []
        excluded = []
        
        source_path = Path(args.source)
        for root, dirs, files in os.walk(source_path):
            root_path = Path(root)
            dirs[:] = [d for d in dirs if should_include(root_path / d, excludes)]
            
            for file in files:
                file_path = root_path / file
                rel_path = file_path.relative_to(source_path)
                if should_include(file_path, excludes):
                    included.append(str(rel_path))
                else:
                    excluded.append(str(rel_path))
        
        print(f"\nWould include {len(included)} files:")
        for f in sorted(included)[:20]:  # Show first 20
            print(f"  ✓ {f}")
        if len(included) > 20:
            print(f"  ... and {len(included) - 20} more files")
        
        print(f"\nWould exclude {len(excluded)} files:")
        for f in sorted(excluded)[:10]:  # Show first 10
            print(f"  ✗ {f}")
        if len(excluded) > 10:
            print(f"  ... and {len(excluded) - 10} more files")
    else:
        # Actually create the zip
        try:
            print("\nCreating zip file...")
            included, excluded = zip_directory(args.source, args.output, excludes)
            
            # Calculate file size
            file_size = Path(args.output).stat().st_size
            size_mb = file_size / (1024 * 1024)
            
            print(f"\n✅ Zip created successfully!")
            print(f"   File: {args.output}")
            print(f"   Size: {size_mb:.2f} MB")
            print(f"   Included: {len(included)} files")
            print(f"   Excluded: {len(excluded)} files")
            
            # Show some key included files
            key_files = ['Cargo.toml', 'README.md', 'WHITEPAPER.md', 'ROADMAP.md']
            print("\nKey files included:")
            for kf in key_files:
                if any(f.endswith(kf) for f in included):
                    print(f"  ✓ {kf}")
            
        except Exception as e:
            print(f"\n❌ Error creating zip: {e}")
            return 1
    
    return 0

if __name__ == '__main__':
    exit(main())
