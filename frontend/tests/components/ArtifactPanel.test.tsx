// tests/components/ArtifactPanel.test.tsx
// Comprehensive tests for ArtifactPanel component

import { describe, it, expect, beforeEach, vi } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { ArtifactPanel } from '../../src/components/ArtifactPanel';
import * as useArtifactsHook from '../../src/hooks/useArtifacts';

// Mock the hooks
vi.mock('../../src/hooks/useArtifacts');
vi.mock('../../src/components/MonacoEditor', () => ({
  MonacoEditor: ({ value, onChange }: any) => (
    <textarea
      data-testid="monaco-editor"
      value={value}
      onChange={(e) => onChange(e.target.value)}
    />
  ),
}));

const mockArtifacts = [
  {
    id: 'art-1',
    path: 'src/components/Button.tsx',
    content: 'export const Button = () => <button>Click</button>;',
    language: 'typescript',
    status: 'draft',
  },
  {
    id: 'art-2',
    path: 'src/utils/helpers.ts',
    content: 'export const add = (a: number, b: number) => a + b;',
    language: 'typescript',
    status: 'saved',
  },
  {
    id: 'art-3',
    path: 'src/main.rs',
    content: 'fn main() { println!("Hello"); }',
    language: 'rust',
    status: 'applied',
  },
];

const defaultMockHook = {
  artifacts: [],
  activeArtifact: null,
  showArtifacts: false,
  addArtifact: vi.fn(),
  setActiveArtifact: vi.fn(),
  updateArtifact: vi.fn(),
  updatePath: vi.fn(),
  removeArtifact: vi.fn(),
  closeArtifacts: vi.fn(),
  save: vi.fn().mockResolvedValue(undefined),
  apply: vi.fn().mockResolvedValue(undefined),
  discard: vi.fn(),
  copyArtifact: vi.fn(),
};

beforeEach(() => {
  vi.clearAllMocks();
  // Mock clipboard
  Object.assign(navigator, {
    clipboard: {
      writeText: vi.fn().mockResolvedValue(undefined),
    },
  });
});

describe('ArtifactPanel Component', () => {
  // ===== Empty State =====
  
  describe('Empty State', () => {
    it('shows empty state when no artifacts', () => {
      vi.mocked(useArtifactsHook.useArtifacts).mockReturnValue(defaultMockHook);
      
      render(<ArtifactPanel />);
      
      expect(screen.getByText('No Artifacts Yet')).toBeInTheDocument();
      expect(screen.getByText(/Ask Mira to create code/)).toBeInTheDocument();
    });
    
    it('displays empty state icon', () => {
      vi.mocked(useArtifactsHook.useArtifacts).mockReturnValue(defaultMockHook);
      
      const { container } = render(<ArtifactPanel />);
      
      // Should have FileText icon
      const icon = container.querySelector('svg');
      expect(icon).toBeInTheDocument();
    });
  });
  
  // ===== Artifact Display =====
  
  describe('Artifact Display', () => {
    it('renders nothing when artifacts exist but none is active', () => {
      vi.mocked(useArtifactsHook.useArtifacts).mockReturnValue({
        ...defaultMockHook,
        artifacts: mockArtifacts,
        activeArtifact: null,
      });
      
      const { container } = render(<ArtifactPanel />);
      
      expect(container.firstChild).toBeNull();
    });
    
    it('displays active artifact in editor', () => {
      vi.mocked(useArtifactsHook.useArtifacts).mockReturnValue({
        ...defaultMockHook,
        artifacts: mockArtifacts,
        activeArtifact: mockArtifacts[0],
      });
      
      render(<ArtifactPanel />);
      
      const editor = screen.getByTestId('monaco-editor');
      expect(editor).toHaveValue(mockArtifacts[0].content);
    });
    
    it('shows artifact path', () => {
      vi.mocked(useArtifactsHook.useArtifacts).mockReturnValue({
        ...defaultMockHook,
        artifacts: mockArtifacts,
        activeArtifact: mockArtifacts[0],
      });
      
      render(<ArtifactPanel />);
      
      expect(screen.getByText('src/components/Button.tsx')).toBeInTheDocument();
    });
  });
  
  // ===== Tab Switching =====
  
  describe('Tab Switching', () => {
    it('displays tabs for all artifacts', () => {
      vi.mocked(useArtifactsHook.useArtifacts).mockReturnValue({
        ...defaultMockHook,
        artifacts: mockArtifacts,
        activeArtifact: mockArtifacts[0],
      });
      
      render(<ArtifactPanel />);
      
      expect(screen.getByText('Button.tsx')).toBeInTheDocument();
      expect(screen.getByText('helpers.ts')).toBeInTheDocument();
      expect(screen.getByText('main.rs')).toBeInTheDocument();
    });
    
    it('highlights active tab', () => {
      vi.mocked(useArtifactsHook.useArtifacts).mockReturnValue({
        ...defaultMockHook,
        artifacts: mockArtifacts,
        activeArtifact: mockArtifacts[0],
      });
      
      const { container } = render(<ArtifactPanel />);
      
      // Active tab should have blue border
      const activeTab = container.querySelector('.border-blue-500');
      expect(activeTab).toBeInTheDocument();
      expect(activeTab).toHaveTextContent('Button.tsx');
    });
    
    it('switches to clicked tab', async () => {
      const setActiveArtifact = vi.fn();
      vi.mocked(useArtifactsHook.useArtifacts).mockReturnValue({
        ...defaultMockHook,
        artifacts: mockArtifacts,
        activeArtifact: mockArtifacts[0],
        setActiveArtifact,
      });
      
      render(<ArtifactPanel />);
      
      const helpersTab = screen.getByText('helpers.ts');
      await userEvent.click(helpersTab);
      
      expect(setActiveArtifact).toHaveBeenCalledWith('art-2');
    });
  });
  
  // ===== Tab Closing =====
  
  describe('Tab Closing', () => {
    it('displays close button on each tab', () => {
      vi.mocked(useArtifactsHook.useArtifacts).mockReturnValue({
        ...defaultMockHook,
        artifacts: mockArtifacts,
        activeArtifact: mockArtifacts[0],
      });
      
      const { container } = render(<ArtifactPanel />);
      
      // Should have X buttons (one per tab)
      const closeButtons = container.querySelectorAll('button[title="Close"]');
      expect(closeButtons).toHaveLength(mockArtifacts.length);
    });
    
    it('removes artifact when close button clicked', async () => {
      const removeArtifact = vi.fn();
      vi.mocked(useArtifactsHook.useArtifacts).mockReturnValue({
        ...defaultMockHook,
        artifacts: mockArtifacts,
        activeArtifact: mockArtifacts[0],
        removeArtifact,
      });
      
      const { container } = render(<ArtifactPanel />);
      
      const closeButtons = container.querySelectorAll('button[title="Close"]');
      await userEvent.click(closeButtons[1]); // Close second tab
      
      expect(removeArtifact).toHaveBeenCalledWith('art-2');
    });
    
    it('stops event propagation when closing tab', async () => {
      const setActiveArtifact = vi.fn();
      const removeArtifact = vi.fn();
      vi.mocked(useArtifactsHook.useArtifacts).mockReturnValue({
        ...defaultMockHook,
        artifacts: mockArtifacts,
        activeArtifact: mockArtifacts[0],
        setActiveArtifact,
        removeArtifact,
      });
      
      const { container } = render(<ArtifactPanel />);
      
      const closeButtons = container.querySelectorAll('button[title="Close"]');
      await userEvent.click(closeButtons[0]);
      
      // Should call removeArtifact but NOT setActiveArtifact
      expect(removeArtifact).toHaveBeenCalled();
      expect(setActiveArtifact).not.toHaveBeenCalled();
    });
  });
  
  // ===== Content Editing =====
  
  describe('Content Editing', () => {
    it('updates artifact content when editor changes', async () => {
      const updateArtifact = vi.fn();
      vi.mocked(useArtifactsHook.useArtifacts).mockReturnValue({
        ...defaultMockHook,
        artifacts: mockArtifacts,
        activeArtifact: mockArtifacts[0],
        updateArtifact,
      });
      
      render(<ArtifactPanel />);
      
      const editor = screen.getByTestId('monaco-editor');
      await userEvent.type(editor, 'x');
      
      // Should call updateArtifact with status: draft
      expect(updateArtifact).toHaveBeenCalled();
      expect(updateArtifact.mock.calls[0][0]).toBe('art-1');
      expect(updateArtifact.mock.calls[0][1]).toMatchObject({
        status: 'draft',
      });
    });
    
    it('marks artifact as draft when content changes', async () => {
      const updateArtifact = vi.fn();
      const savedArtifact = { ...mockArtifacts[1], status: 'saved' };
      
      vi.mocked(useArtifactsHook.useArtifacts).mockReturnValue({
        ...defaultMockHook,
        artifacts: [savedArtifact],
        activeArtifact: savedArtifact,
        updateArtifact,
      });
      
      render(<ArtifactPanel />);
      
      const editor = screen.getByTestId('monaco-editor');
      await userEvent.type(editor, 'x');
      
      expect(updateArtifact).toHaveBeenCalledWith('art-2', expect.objectContaining({
        status: 'draft',
      }));
    });
  });
  
  // ===== Path Editing =====
  
  describe('Path Editing', () => {
    it('allows editing path when clicked', async () => {
      vi.mocked(useArtifactsHook.useArtifacts).mockReturnValue({
        ...defaultMockHook,
        artifacts: mockArtifacts,
        activeArtifact: mockArtifacts[0],
      });
      
      render(<ArtifactPanel />);
      
      const pathButton = screen.getByText('src/components/Button.tsx');
      await userEvent.click(pathButton);
      
      // Should show input field
      const input = screen.getByDisplayValue('src/components/Button.tsx');
      expect(input).toBeInTheDocument();
      expect(input.tagName).toBe('INPUT');
    });
    
    it('saves path on Enter key', async () => {
      const updatePath = vi.fn();
      vi.mocked(useArtifactsHook.useArtifacts).mockReturnValue({
        ...defaultMockHook,
        artifacts: mockArtifacts,
        activeArtifact: mockArtifacts[0],
        updatePath,
      });
      
      render(<ArtifactPanel />);
      
      const pathButton = screen.getByText('src/components/Button.tsx');
      await userEvent.click(pathButton);
      
      const input = screen.getByDisplayValue('src/components/Button.tsx');
      await userEvent.clear(input);
      await userEvent.type(input, 'src/NewButton.tsx');
      fireEvent.keyDown(input, { key: 'Enter' });
      
      expect(updatePath).toHaveBeenCalledWith('art-1', 'src/NewButton.tsx');
    });
    
    it('cancels path edit on Escape key', async () => {
      const updatePath = vi.fn();
      vi.mocked(useArtifactsHook.useArtifacts).mockReturnValue({
        ...defaultMockHook,
        artifacts: mockArtifacts,
        activeArtifact: mockArtifacts[0],
        updatePath,
      });
      
      render(<ArtifactPanel />);
      
      const pathButton = screen.getByText('src/components/Button.tsx');
      await userEvent.click(pathButton);
      
      const input = screen.getByDisplayValue('src/components/Button.tsx');
      await userEvent.clear(input);
      await userEvent.type(input, 'different path');
      fireEvent.keyDown(input, { key: 'Escape' });
      
      expect(updatePath).not.toHaveBeenCalled();
      // Should revert to original path
      await waitFor(() => {
        expect(screen.getByText('src/components/Button.tsx')).toBeInTheDocument();
      });
    });
    
    it('saves path on blur', async () => {
      const updatePath = vi.fn();
      vi.mocked(useArtifactsHook.useArtifacts).mockReturnValue({
        ...defaultMockHook,
        artifacts: mockArtifacts,
        activeArtifact: mockArtifacts[0],
        updatePath,
      });
      
      render(<ArtifactPanel />);
      
      const pathButton = screen.getByText('src/components/Button.tsx');
      await userEvent.click(pathButton);
      
      const input = screen.getByDisplayValue('src/components/Button.tsx');
      await userEvent.clear(input);
      await userEvent.type(input, 'new/path.tsx');
      fireEvent.blur(input);
      
      expect(updatePath).toHaveBeenCalledWith('art-1', 'new/path.tsx');
    });
  });
  
  // ===== Action Buttons =====
  
  describe('Action Buttons', () => {
    it('displays Copy, Save, Apply, and Close buttons', () => {
      vi.mocked(useArtifactsHook.useArtifacts).mockReturnValue({
        ...defaultMockHook,
        artifacts: mockArtifacts,
        activeArtifact: mockArtifacts[0],
      });
      
      render(<ArtifactPanel />);
      
      expect(screen.getByTitle(/Copy to clipboard/i)).toBeInTheDocument();
      expect(screen.getByText('Save')).toBeInTheDocument();
      expect(screen.getByText('Apply')).toBeInTheDocument();
      expect(screen.getByTitle(/Close panel/i)).toBeInTheDocument();
    });
    
    it('copies artifact content to clipboard', async () => {
      const copyArtifact = vi.fn();
      vi.mocked(useArtifactsHook.useArtifacts).mockReturnValue({
        ...defaultMockHook,
        artifacts: mockArtifacts,
        activeArtifact: mockArtifacts[0],
        copyArtifact,
      });
      
      render(<ArtifactPanel />);
      
      const copyButton = screen.getByTitle(/Copy to clipboard/i);
      await userEvent.click(copyButton);
      
      expect(copyArtifact).toHaveBeenCalledWith('art-1');
    });
    
    it('saves artifact when Save button clicked', async () => {
      const save = vi.fn().mockResolvedValue(undefined);
      vi.mocked(useArtifactsHook.useArtifacts).mockReturnValue({
        ...defaultMockHook,
        artifacts: mockArtifacts,
        activeArtifact: mockArtifacts[0],
        save,
      });
      
      render(<ArtifactPanel />);
      
      const saveButton = screen.getByText('Save');
      await userEvent.click(saveButton);
      
      expect(save).toHaveBeenCalledWith('art-1');
    });
    
    it('applies artifact when Apply button clicked', async () => {
      const apply = vi.fn().mockResolvedValue(undefined);
      vi.mocked(useArtifactsHook.useArtifacts).mockReturnValue({
        ...defaultMockHook,
        artifacts: mockArtifacts,
        activeArtifact: mockArtifacts[0],
        apply,
      });
      
      render(<ArtifactPanel />);
      
      const applyButton = screen.getByText('Apply');
      await userEvent.click(applyButton);
      
      expect(apply).toHaveBeenCalledWith('art-1');
    });
    
    it('closes panel when Close button clicked', async () => {
      const closeArtifacts = vi.fn();
      vi.mocked(useArtifactsHook.useArtifacts).mockReturnValue({
        ...defaultMockHook,
        artifacts: mockArtifacts,
        activeArtifact: mockArtifacts[0],
        closeArtifacts,
      });
      
      render(<ArtifactPanel />);
      
      const closeButton = screen.getByTitle(/Close panel/i);
      await userEvent.click(closeButton);
      
      expect(closeArtifacts).toHaveBeenCalled();
    });
  });
  
  // ===== Toast Notifications =====
  
  describe('Toast Notifications', () => {
    it('shows success toast after successful save', async () => {
      const save = vi.fn().mockResolvedValue(undefined);
      vi.mocked(useArtifactsHook.useArtifacts).mockReturnValue({
        ...defaultMockHook,
        artifacts: mockArtifacts,
        activeArtifact: mockArtifacts[0],
        save,
      });
      
      render(<ArtifactPanel />);
      
      const saveButton = screen.getByText('Save');
      await userEvent.click(saveButton);
      
      await waitFor(() => {
        expect(screen.getByText(/Saved src\/components\/Button\.tsx/)).toBeInTheDocument();
      });
    });
    
    it('shows error toast when save fails', async () => {
      const save = vi.fn().mockRejectedValue(new Error('Save failed'));
      vi.mocked(useArtifactsHook.useArtifacts).mockReturnValue({
        ...defaultMockHook,
        artifacts: mockArtifacts,
        activeArtifact: mockArtifacts[0],
        save,
      });
      
      render(<ArtifactPanel />);
      
      const saveButton = screen.getByText('Save');
      await userEvent.click(saveButton);
      
      await waitFor(() => {
        expect(screen.getByText(/Failed to save/)).toBeInTheDocument();
      });
    });
    
    it('shows success toast after successful apply', async () => {
      const apply = vi.fn().mockResolvedValue(undefined);
      vi.mocked(useArtifactsHook.useArtifacts).mockReturnValue({
        ...defaultMockHook,
        artifacts: mockArtifacts,
        activeArtifact: mockArtifacts[0],
        apply,
      });
      
      render(<ArtifactPanel />);
      
      const applyButton = screen.getByText('Apply');
      await userEvent.click(applyButton);
      
      await waitFor(() => {
        expect(screen.getByText(/Applied.*to workspace/)).toBeInTheDocument();
      });
    });
    
    it('shows info toast when copying to clipboard', async () => {
      const copyArtifact = vi.fn();
      vi.mocked(useArtifactsHook.useArtifacts).mockReturnValue({
        ...defaultMockHook,
        artifacts: mockArtifacts,
        activeArtifact: mockArtifacts[0],
        copyArtifact,
      });
      
      render(<ArtifactPanel />);
      
      const copyButton = screen.getByTitle(/Copy to clipboard/i);
      await userEvent.click(copyButton);
      
      await waitFor(() => {
        expect(screen.getByText('Copied to clipboard')).toBeInTheDocument();
      });
    });
    
    it('displays multiple toasts simultaneously', async () => {
      const save = vi.fn().mockResolvedValue(undefined);
      const apply = vi.fn().mockResolvedValue(undefined);
      
      vi.mocked(useArtifactsHook.useArtifacts).mockReturnValue({
        ...defaultMockHook,
        artifacts: mockArtifacts,
        activeArtifact: mockArtifacts[0],
        save,
        apply,
      });
      
      render(<ArtifactPanel />);
      
      const saveButton = screen.getByText('Save');
      const applyButton = screen.getByText('Apply');
      
      await userEvent.click(saveButton);
      await userEvent.click(applyButton);
      
      // Check for toast-specific messages (not the status badges)
      await waitFor(() => {
        expect(screen.getByText(/Saved src\/components\/Button\.tsx$/)).toBeInTheDocument();
        expect(screen.getByText(/Applied.*to workspace/)).toBeInTheDocument();
      });
    });
  });
  
  // ===== Keyboard Shortcuts =====
  
  describe('Keyboard Shortcuts', () => {
    it('saves on Cmd+S', async () => {
      const save = vi.fn().mockResolvedValue(undefined);
      vi.mocked(useArtifactsHook.useArtifacts).mockReturnValue({
        ...defaultMockHook,
        artifacts: mockArtifacts,
        activeArtifact: mockArtifacts[0],
        save,
      });
      
      render(<ArtifactPanel />);
      
      fireEvent.keyDown(window, { key: 's', metaKey: true });
      
      expect(save).toHaveBeenCalledWith('art-1');
    });
    
    it('saves on Ctrl+S', async () => {
      const save = vi.fn().mockResolvedValue(undefined);
      vi.mocked(useArtifactsHook.useArtifacts).mockReturnValue({
        ...defaultMockHook,
        artifacts: mockArtifacts,
        activeArtifact: mockArtifacts[0],
        save,
      });
      
      render(<ArtifactPanel />);
      
      fireEvent.keyDown(window, { key: 's', ctrlKey: true });
      
      expect(save).toHaveBeenCalledWith('art-1');
    });
    
    it('applies on Cmd+Enter', async () => {
      const apply = vi.fn().mockResolvedValue(undefined);
      vi.mocked(useArtifactsHook.useArtifacts).mockReturnValue({
        ...defaultMockHook,
        artifacts: mockArtifacts,
        activeArtifact: mockArtifacts[0],
        apply,
      });
      
      render(<ArtifactPanel />);
      
      fireEvent.keyDown(window, { key: 'Enter', metaKey: true });
      
      expect(apply).toHaveBeenCalledWith('art-1');
    });
    
    it('applies on Ctrl+Enter', async () => {
      const apply = vi.fn().mockResolvedValue(undefined);
      vi.mocked(useArtifactsHook.useArtifacts).mockReturnValue({
        ...defaultMockHook,
        artifacts: mockArtifacts,
        activeArtifact: mockArtifacts[0],
        apply,
      });
      
      render(<ArtifactPanel />);
      
      fireEvent.keyDown(window, { key: 'Enter', ctrlKey: true });
      
      expect(apply).toHaveBeenCalledWith('art-1');
    });
    
    it('prevents default browser behavior on Cmd+S', async () => {
      const save = vi.fn().mockResolvedValue(undefined);
      vi.mocked(useArtifactsHook.useArtifacts).mockReturnValue({
        ...defaultMockHook,
        artifacts: mockArtifacts,
        activeArtifact: mockArtifacts[0],
        save,
      });
      
      render(<ArtifactPanel />);
      
      const event = new KeyboardEvent('keydown', { key: 's', metaKey: true });
      const preventDefaultSpy = vi.spyOn(event, 'preventDefault');
      
      fireEvent(window, event);
      
      expect(preventDefaultSpy).toHaveBeenCalled();
    });
  });
  
  // ===== Status Badges =====
  
  describe('Status Badges', () => {
    it('shows no badge for draft status', () => {
      const draftArtifact = { ...mockArtifacts[0], status: 'draft' };
      vi.mocked(useArtifactsHook.useArtifacts).mockReturnValue({
        ...defaultMockHook,
        artifacts: [draftArtifact],
        activeArtifact: draftArtifact,
      });
      
      const { container } = render(<ArtifactPanel />);
      
      // Should not have status badge for draft
      const badges = container.querySelectorAll('.bg-green-900, .bg-blue-900');
      expect(badges).toHaveLength(0);
    });
    
    it('shows "Saved" badge for saved status', () => {
      const savedArtifact = { ...mockArtifacts[1], status: 'saved' };
      vi.mocked(useArtifactsHook.useArtifacts).mockReturnValue({
        ...defaultMockHook,
        artifacts: [savedArtifact],
        activeArtifact: savedArtifact,
      });
      
      render(<ArtifactPanel />);
      
      expect(screen.getByText('Saved')).toBeInTheDocument();
    });
    
    it('shows "Applied" badge for applied status', () => {
      const appliedArtifact = { ...mockArtifacts[2], status: 'applied' };
      vi.mocked(useArtifactsHook.useArtifacts).mockReturnValue({
        ...defaultMockHook,
        artifacts: [appliedArtifact],
        activeArtifact: appliedArtifact,
      });
      
      render(<ArtifactPanel />);
      
      expect(screen.getByText('Applied')).toBeInTheDocument();
    });
  });
  
  // ===== Language Icons =====
  
  describe('Language Icons', () => {
    it('shows Code icon for code files', () => {
      vi.mocked(useArtifactsHook.useArtifacts).mockReturnValue({
        ...defaultMockHook,
        artifacts: mockArtifacts,
        activeArtifact: mockArtifacts[0],
      });
      
      const { container } = render(<ArtifactPanel />);
      
      // Should have Code icons for typescript/rust files
      const icons = container.querySelectorAll('svg');
      expect(icons.length).toBeGreaterThan(0);
    });
    
    it('shows FileText icon for non-code files', () => {
      const textArtifact = {
        id: 'art-4',
        path: 'README.txt',
        content: 'Hello world',
        language: 'plaintext',
      };
      
      vi.mocked(useArtifactsHook.useArtifacts).mockReturnValue({
        ...defaultMockHook,
        artifacts: [textArtifact],
        activeArtifact: textArtifact,
      });
      
      const { container } = render(<ArtifactPanel />);
      
      const icons = container.querySelectorAll('svg');
      expect(icons.length).toBeGreaterThan(0);
    });
  });
  
  // ===== Edge Cases =====
  
  describe('Edge Cases', () => {
    it('handles switching artifacts without errors', async () => {
      const setActiveArtifact = vi.fn();
      vi.mocked(useArtifactsHook.useArtifacts).mockReturnValue({
        ...defaultMockHook,
        artifacts: mockArtifacts,
        activeArtifact: mockArtifacts[0],
        setActiveArtifact,
      });
      
      const { rerender } = render(<ArtifactPanel />);
      
      // Switch to second artifact
      vi.mocked(useArtifactsHook.useArtifacts).mockReturnValue({
        ...defaultMockHook,
        artifacts: mockArtifacts,
        activeArtifact: mockArtifacts[1],
        setActiveArtifact,
      });
      
      rerender(<ArtifactPanel />);
      
      const editor = screen.getByTestId('monaco-editor');
      expect(editor).toHaveValue(mockArtifacts[1].content);
    });
    
    it('handles empty path gracefully', () => {
      const emptyPathArtifact = { ...mockArtifacts[0], path: '' };
      vi.mocked(useArtifactsHook.useArtifacts).mockReturnValue({
        ...defaultMockHook,
        artifacts: [emptyPathArtifact],
        activeArtifact: emptyPathArtifact,
      });
      
      expect(() => render(<ArtifactPanel />)).not.toThrow();
    });
    
    it('handles very long file paths', () => {
      const longPathArtifact = {
        ...mockArtifacts[0],
        path: 'src/very/deep/nested/directory/structure/with/many/levels/component.tsx',
      };
      vi.mocked(useArtifactsHook.useArtifacts).mockReturnValue({
        ...defaultMockHook,
        artifacts: [longPathArtifact],
        activeArtifact: longPathArtifact,
      });
      
      render(<ArtifactPanel />);
      
      // Should show truncated filename
      expect(screen.getByText('component.tsx')).toBeInTheDocument();
    });
    
    it('handles artifacts with no language specified', () => {
      const noLangArtifact = {
        id: 'art-5',
        path: 'unknown',
        content: 'content',
        language: undefined,
      };
      
      vi.mocked(useArtifactsHook.useArtifacts).mockReturnValue({
        ...defaultMockHook,
        artifacts: [noLangArtifact],
        activeArtifact: noLangArtifact,
      });
      
      expect(() => render(<ArtifactPanel />)).not.toThrow();
    });
  });
});
