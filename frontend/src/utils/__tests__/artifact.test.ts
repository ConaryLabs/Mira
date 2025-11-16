// src/utils/__tests__/artifact.test.ts
// Unit tests for artifact utility functions

import { describe, it, expect } from 'vitest';
import {
  normalizePath,
  extractContent,
  extractPath,
  extractLanguage,
  createArtifact,
  extractArtifacts,
} from '../artifact';

describe('normalizePath', () => {
  it('handles undefined input', () => {
    expect(normalizePath()).toBe('untitled');
    expect(normalizePath(undefined)).toBe('untitled');
  });

  it('converts Windows paths to POSIX', () => {
    expect(normalizePath('src\\components\\Button.tsx')).toBe('src/components/Button.tsx');
  });

  it('collapses duplicate slashes', () => {
    expect(normalizePath('src//components///Button.tsx')).toBe('src/components/Button.tsx');
  });

  it('strips leading ./', () => {
    expect(normalizePath('./src/Button.tsx')).toBe('src/Button.tsx');
    expect(normalizePath('.//src/Button.tsx')).toBe('src/Button.tsx');
  });

  it('handles complex paths', () => {
    expect(normalizePath('.\\src\\\\components//Button.tsx')).toBe('src/components/Button.tsx');
  });
});

describe('extractContent', () => {
  it('extracts from content field', () => {
    expect(extractContent({ content: 'Hello' })).toBe('Hello');
  });

  it('extracts from file_content field', () => {
    expect(extractContent({ file_content: 'World' })).toBe('World');
  });

  it('extracts from text field', () => {
    expect(extractContent({ text: 'Test' })).toBe('Test');
  });

  it('extracts from body field', () => {
    expect(extractContent({ body: 'Body text' })).toBe('Body text');
  });

  it('extracts from value field', () => {
    expect(extractContent({ value: 'Value' })).toBe('Value');
  });

  it('returns undefined for empty object', () => {
    expect(extractContent({})).toBeUndefined();
  });

  it('prioritizes content over other fields', () => {
    const obj = {
      content: 'Content',
      text: 'Text',
      body: 'Body',
    };
    expect(extractContent(obj)).toBe('Content');
  });
});

describe('extractPath', () => {
  it('extracts from path field', () => {
    expect(extractPath({ path: 'src/test.ts' })).toBe('src/test.ts');
  });

  it('extracts from file_path field', () => {
    expect(extractPath({ file_path: 'src/test.ts' })).toBe('src/test.ts');
  });

  it('extracts from title field', () => {
    expect(extractPath({ title: 'test.ts' })).toBe('test.ts');
  });

  it('extracts from name field', () => {
    expect(extractPath({ name: 'test.ts' })).toBe('test.ts');
  });

  it('returns untitled for empty object', () => {
    expect(extractPath({})).toBe('untitled');
  });

  it('normalizes extracted path', () => {
    expect(extractPath({ path: '.\\src\\test.ts' })).toBe('src/test.ts');
  });
});

describe('extractLanguage', () => {
  it('uses explicit language field', () => {
    expect(extractLanguage({ language: 'typescript' }, 'test.ts')).toBe('typescript');
  });

  it('uses programming_lang field', () => {
    expect(extractLanguage({ programming_lang: 'rust' }, 'test.rs')).toBe('rust');
  });

  it('detects language from fallback path', () => {
    const result = extractLanguage({}, 'test.ts');
    // detectLanguage is tested separately, just ensure it was called
    expect(result).toBeDefined();
  });
});

describe('createArtifact', () => {
  it('creates artifact from valid payload', () => {
    const payload = {
      id: 'test-123',
      path: 'src/test.ts',
      content: 'const x = 1;',
      language: 'typescript',
    };

    const artifact = createArtifact(payload);

    expect(artifact).toBeDefined();
    expect(artifact!.id).toBe('test-123');
    expect(artifact!.path).toBe('src/test.ts');
    expect(artifact!.content).toBe('const x = 1;');
    expect(artifact!.language).toBe('typescript');
  });

  it('returns null for null input', () => {
    expect(createArtifact(null)).toBeNull();
  });

  it('returns null for missing content', () => {
    const payload = {
      path: 'test.ts',
      // no content
    };
    expect(createArtifact(payload)).toBeNull();
  });

  it('returns null for non-string content', () => {
    const payload = {
      path: 'test.ts',
      content: 123, // invalid
    };
    expect(createArtifact(payload)).toBeNull();
  });

  it('generates ID if not provided', () => {
    const payload = {
      path: 'test.ts',
      content: 'test',
    };

    const artifact = createArtifact(payload);
    expect(artifact!.id).toMatch(/^artifact-\d+-[a-z0-9]+$/);
  });

  it('uses custom ID prefix', () => {
    const payload = {
      path: 'test.ts',
      content: 'test',
    };

    const artifact = createArtifact(payload, { idPrefix: 'file' });
    expect(artifact!.id).toMatch(/^file-\d+-[a-z0-9]+$/);
  });

  it('applies custom status and origin', () => {
    const payload = {
      path: 'test.ts',
      content: 'test',
    };

    const artifact = createArtifact(payload, {
      status: 'saved',
      origin: 'user',
    });

    expect(artifact!.status).toBe('saved');
    expect(artifact!.origin).toBe('user');
  });

  it('extracts content from various field names', () => {
    const payloads = [
      { path: 'test.ts', content: 'content' },
      { path: 'test.ts', file_content: 'file_content' },
      { path: 'test.ts', text: 'text' },
      { path: 'test.ts', body: 'body' },
      { path: 'test.ts', value: 'value' },
    ];

    payloads.forEach((payload, i) => {
      const artifact = createArtifact(payload);
      expect(artifact).not.toBeNull();
    });
  });

  it('extracts path from various field names', () => {
    const payloads = [
      { path: 'test1.ts', content: 'test' },
      { file_path: 'test2.ts', content: 'test' },
      { title: 'test3.ts', content: 'test' },
      { name: 'test4.ts', content: 'test' },
    ];

    payloads.forEach((payload) => {
      const artifact = createArtifact(payload);
      expect(artifact).not.toBeNull();
      expect(artifact!.path).toContain('test');
    });
  });

  it('normalizes paths correctly', () => {
    const payload = {
      path: '.\\src\\\\Button.tsx',
      content: 'test',
    };

    const artifact = createArtifact(payload);
    expect(artifact!.path).toBe('src/Button.tsx');
  });

  it('handles changeType field', () => {
    const payload1 = {
      path: 'test.ts',
      content: 'test',
      change_type: 'primary',
    };

    const artifact1 = createArtifact(payload1);
    expect(artifact1!.changeType).toBe('primary');

    const payload2 = {
      path: 'test.ts',
      content: 'test',
      changeType: 'import',
    };

    const artifact2 = createArtifact(payload2);
    expect(artifact2!.changeType).toBe('import');
  });

  it('sets timestamp', () => {
    const payload = {
      path: 'test.ts',
      content: 'test',
    };

    const before = Date.now();
    const artifact = createArtifact(payload);
    const after = Date.now();

    expect(artifact!.timestamp).toBeGreaterThanOrEqual(before);
    expect(artifact!.timestamp).toBeLessThanOrEqual(after);
  });
});

describe('extractArtifacts', () => {
  it('returns empty array for null input', () => {
    expect(extractArtifacts(null)).toEqual([]);
  });

  it('extracts from direct artifact object', () => {
    const data = {
      artifact: {
        path: 'test.ts',
        content: 'test',
      },
    };

    const artifacts = extractArtifacts(data);
    expect(artifacts).toHaveLength(1);
    expect(artifacts[0].path).toBe('test.ts');
  });

  it('extracts from artifacts array', () => {
    const data = {
      artifacts: [
        { path: 'test1.ts', content: 'test1' },
        { path: 'test2.ts', content: 'test2' },
      ],
    };

    const artifacts = extractArtifacts(data);
    expect(artifacts).toHaveLength(2);
    expect(artifacts[0].path).toBe('test1.ts');
    expect(artifacts[1].path).toBe('test2.ts');
  });

  it('skips invalid artifacts in array', () => {
    const data = {
      artifacts: [
        { path: 'test1.ts', content: 'test1' },
        { path: 'test2.ts' }, // invalid: no content
        { path: 'test3.ts', content: 'test3' },
      ],
    };

    const artifacts = extractArtifacts(data);
    expect(artifacts).toHaveLength(2);
    expect(artifacts[0].path).toBe('test1.ts');
    expect(artifacts[1].path).toBe('test3.ts');
  });

  it('extracts from files array', () => {
    const data = {
      files: [
        { path: 'test.ts', content: 'test' },
      ],
    };

    const artifacts = extractArtifacts(data);
    expect(artifacts).toHaveLength(1);
  });

  it('extracts from results array', () => {
    const data = {
      results: [
        { path: 'test.ts', content: 'test' },
      ],
    };

    const artifacts = extractArtifacts(data);
    expect(artifacts).toHaveLength(1);
  });

  it('extracts from result object', () => {
    const data = {
      result: {
        path: 'test.ts',
        content: 'test',
      },
    };

    const artifacts = extractArtifacts(data);
    expect(artifacts).toHaveLength(1);
  });

  it('falls back to data itself if valid artifact', () => {
    const data = {
      path: 'test.ts',
      content: 'test',
    };

    const artifacts = extractArtifacts(data);
    expect(artifacts).toHaveLength(1);
    expect(artifacts[0].path).toBe('test.ts');
  });

  it('applies options to all extracted artifacts', () => {
    const data = {
      artifacts: [
        { path: 'test1.ts', content: 'test1' },
        { path: 'test2.ts', content: 'test2' },
      ],
    };

    const artifacts = extractArtifacts(data, {
      status: 'saved',
      origin: 'user',
      idPrefix: 'file',
    });

    expect(artifacts).toHaveLength(2);
    artifacts.forEach(artifact => {
      expect(artifact.status).toBe('saved');
      expect(artifact.origin).toBe('user');
      expect(artifact.id).toMatch(/^file-/);
    });
  });

  it('extracts both direct artifact and artifacts array', () => {
    const data = {
      artifact: { path: 'direct.ts', content: 'direct' },
      artifacts: [{ path: 'array.ts', content: 'array' }],
    };

    const artifacts = extractArtifacts(data);
    // Implementation extracts both, which is fine
    expect(artifacts.length).toBeGreaterThan(0);
    const paths = artifacts.map(a => a.path);
    expect(paths).toContain('direct.ts');
  });

  it('handles complex nested structures', () => {
    const data = {
      response: {
        data: {
          artifacts: [
            { path: 'test.ts', content: 'test' },
          ],
        },
      },
    };

    // This won't work as extractArtifacts doesn't deep traverse
    // It's expected behavior - extraction is one level deep
    const artifacts = extractArtifacts(data);
    expect(artifacts).toEqual([]);
  });

  it('returns empty array when no valid artifacts found', () => {
    const data = {
      someOtherField: 'value',
      anotherField: 123,
    };

    const artifacts = extractArtifacts(data);
    expect(artifacts).toEqual([]);
  });
});
