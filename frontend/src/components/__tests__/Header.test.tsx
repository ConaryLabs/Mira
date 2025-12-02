// src/components/__tests__/Header.test.tsx
// Header Component Tests

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { Header } from '../Header';
import { useAppState, useArtifactState } from '../../stores/useAppState';
import { useActivityStore } from '../../stores/useActivityStore';
import { useCodeIntelligenceStore } from '../../stores/useCodeIntelligenceStore';
import { useAuthStore, useCurrentUser } from '../../stores/useAuthStore';

// Mock react-router-dom
const mockNavigate = vi.fn();
vi.mock('react-router-dom', () => ({
  useNavigate: () => mockNavigate,
}));

// Mock the stores
vi.mock('../../stores/useAppState', () => ({
  useAppState: vi.fn(),
  useArtifactState: vi.fn(),
}));

vi.mock('../../stores/useActivityStore', () => ({
  useActivityStore: vi.fn(),
}));

vi.mock('../../stores/useCodeIntelligenceStore', () => ({
  useCodeIntelligenceStore: vi.fn(),
}));

vi.mock('../../stores/useAuthStore', () => ({
  useAuthStore: vi.fn(),
  useCurrentUser: vi.fn(),
}));

// Mock child components
vi.mock('../ArtifactToggle', () => ({
  default: ({ isOpen, onClick, artifactCount }: any) => (
    <button data-testid="artifact-toggle" onClick={onClick}>
      Artifacts ({artifactCount})
    </button>
  ),
}));

vi.mock('../ProjectsView', () => ({
  ProjectsView: () => <div data-testid="projects-view">Projects View</div>,
}));

describe('Header', () => {
  let mockSetShowArtifacts: ReturnType<typeof vi.fn>;
  let mockToggleActivityPanel: ReturnType<typeof vi.fn>;
  let mockToggleIntelligencePanel: ReturnType<typeof vi.fn>;
  let mockLogout: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    vi.clearAllMocks();

    mockSetShowArtifacts = vi.fn();
    mockToggleActivityPanel = vi.fn();
    mockToggleIntelligencePanel = vi.fn();
    mockLogout = vi.fn();

    // Default useAppState mock
    (useAppState as unknown as ReturnType<typeof vi.fn>).mockReturnValue({
      currentProject: null,
      showArtifacts: false,
      setShowArtifacts: mockSetShowArtifacts,
    });

    // Default useArtifactState mock
    (useArtifactState as unknown as ReturnType<typeof vi.fn>).mockReturnValue({
      artifacts: [],
    });

    // Default useActivityStore mock
    (useActivityStore as unknown as ReturnType<typeof vi.fn>).mockReturnValue({
      togglePanel: mockToggleActivityPanel,
      isPanelVisible: false,
    });

    // Default useCodeIntelligenceStore mock
    (useCodeIntelligenceStore as unknown as ReturnType<typeof vi.fn>).mockReturnValue({
      togglePanel: mockToggleIntelligencePanel,
      isPanelVisible: false,
    });

    // Default useAuthStore mock
    (useAuthStore as unknown as ReturnType<typeof vi.fn>).mockReturnValue({
      logout: mockLogout,
    });

    // Default useCurrentUser mock
    (useCurrentUser as unknown as ReturnType<typeof vi.fn>).mockReturnValue(null);
  });

  describe('project selector', () => {
    it('shows "No Project" when no project is selected', () => {
      render(<Header />);

      expect(screen.getByText('No Project')).toBeInTheDocument();
    });

    it('shows project name when a project is selected', () => {
      (useAppState as unknown as ReturnType<typeof vi.fn>).mockReturnValue({
        currentProject: { id: 'proj-1', name: 'My Awesome Project' },
        showArtifacts: false,
        setShowArtifacts: mockSetShowArtifacts,
      });

      render(<Header />);

      expect(screen.getByText('My Awesome Project')).toBeInTheDocument();
    });

    it('opens projects modal when project button is clicked', () => {
      render(<Header />);

      const projectButton = screen.getByTitle('Manage Projects');
      fireEvent.click(projectButton);

      expect(screen.getByTestId('projects-view')).toBeInTheDocument();
      expect(screen.getByText('Projects')).toBeInTheDocument();
    });

    it('closes projects modal when close button is clicked', () => {
      render(<Header />);

      // Open modal
      const projectButton = screen.getByTitle('Manage Projects');
      fireEvent.click(projectButton);

      expect(screen.getByTestId('projects-view')).toBeInTheDocument();

      // Close modal
      const closeButton = screen.getByTitle('Close');
      fireEvent.click(closeButton);

      expect(screen.queryByTestId('projects-view')).not.toBeInTheDocument();
    });
  });

  describe('user display', () => {
    it('shows display name when user is logged in', () => {
      (useCurrentUser as unknown as ReturnType<typeof vi.fn>).mockReturnValue({
        id: 'user-1',
        username: 'testuser',
        displayName: 'Test User',
      });

      render(<Header />);

      expect(screen.getByText('Test User')).toBeInTheDocument();
    });

    it('shows username when displayName is not available', () => {
      (useCurrentUser as unknown as ReturnType<typeof vi.fn>).mockReturnValue({
        id: 'user-1',
        username: 'testuser',
        displayName: null,
      });

      render(<Header />);

      expect(screen.getByText('testuser')).toBeInTheDocument();
    });

    it('does not show user info when not logged in', () => {
      (useCurrentUser as unknown as ReturnType<typeof vi.fn>).mockReturnValue(null);

      const { container } = render(<Header />);

      // Check that there's no user info element
      expect(container.querySelector('.text-gray-400:not([title])')).toBeNull();
    });
  });

  describe('panel toggle buttons', () => {
    it('does not show panel toggles when no project is selected', () => {
      render(<Header />);

      expect(screen.queryByTitle('Toggle Activity Panel')).not.toBeInTheDocument();
      expect(screen.queryByTitle(/Toggle Intelligence Panel/)).not.toBeInTheDocument();
    });

    it('shows panel toggles when a project is selected', () => {
      (useAppState as unknown as ReturnType<typeof vi.fn>).mockReturnValue({
        currentProject: { id: 'proj-1', name: 'Test Project' },
        showArtifacts: false,
        setShowArtifacts: mockSetShowArtifacts,
      });

      render(<Header />);

      expect(screen.getByTitle('Toggle Activity Panel')).toBeInTheDocument();
      expect(screen.getByTitle(/Toggle Intelligence Panel/)).toBeInTheDocument();
    });

    it('calls togglePanel for activity when clicked', () => {
      (useAppState as unknown as ReturnType<typeof vi.fn>).mockReturnValue({
        currentProject: { id: 'proj-1', name: 'Test Project' },
        showArtifacts: false,
        setShowArtifacts: mockSetShowArtifacts,
      });

      render(<Header />);

      const activityButton = screen.getByTitle('Toggle Activity Panel');
      fireEvent.click(activityButton);

      expect(mockToggleActivityPanel).toHaveBeenCalledTimes(1);
    });

    it('calls togglePanel for intelligence when clicked', () => {
      (useAppState as unknown as ReturnType<typeof vi.fn>).mockReturnValue({
        currentProject: { id: 'proj-1', name: 'Test Project' },
        showArtifacts: false,
        setShowArtifacts: mockSetShowArtifacts,
      });

      render(<Header />);

      const intelligenceButton = screen.getByTitle(/Toggle Intelligence Panel/);
      fireEvent.click(intelligenceButton);

      expect(mockToggleIntelligencePanel).toHaveBeenCalledTimes(1);
    });

    it('shows active state when activity panel is visible', () => {
      (useAppState as unknown as ReturnType<typeof vi.fn>).mockReturnValue({
        currentProject: { id: 'proj-1', name: 'Test Project' },
        showArtifacts: false,
        setShowArtifacts: mockSetShowArtifacts,
      });

      (useActivityStore as unknown as ReturnType<typeof vi.fn>).mockReturnValue({
        togglePanel: mockToggleActivityPanel,
        isPanelVisible: true,
      });

      render(<Header />);

      const activityButton = screen.getByTitle('Toggle Activity Panel');
      expect(activityButton.className).toContain('text-blue-400');
      expect(activityButton.className).toContain('bg-blue-900/30');
    });

    it('shows active state when intelligence panel is visible', () => {
      (useAppState as unknown as ReturnType<typeof vi.fn>).mockReturnValue({
        currentProject: { id: 'proj-1', name: 'Test Project' },
        showArtifacts: false,
        setShowArtifacts: mockSetShowArtifacts,
      });

      (useCodeIntelligenceStore as unknown as ReturnType<typeof vi.fn>).mockReturnValue({
        togglePanel: mockToggleIntelligencePanel,
        isPanelVisible: true,
      });

      render(<Header />);

      const intelligenceButton = screen.getByTitle(/Toggle Intelligence Panel/);
      expect(intelligenceButton.className).toContain('text-purple-400');
      expect(intelligenceButton.className).toContain('bg-purple-900/30');
    });
  });

  describe('artifact toggle', () => {
    it('does not show artifact toggle when no project and no artifacts', () => {
      render(<Header />);

      expect(screen.queryByTestId('artifact-toggle')).not.toBeInTheDocument();
    });

    it('shows artifact toggle when project is selected', () => {
      (useAppState as unknown as ReturnType<typeof vi.fn>).mockReturnValue({
        currentProject: { id: 'proj-1', name: 'Test Project' },
        showArtifacts: false,
        setShowArtifacts: mockSetShowArtifacts,
      });

      render(<Header />);

      expect(screen.getByTestId('artifact-toggle')).toBeInTheDocument();
    });

    it('shows artifact toggle when artifacts exist', () => {
      (useArtifactState as unknown as ReturnType<typeof vi.fn>).mockReturnValue({
        artifacts: [{ id: 'art-1', content: 'test' }],
      });

      render(<Header />);

      expect(screen.getByTestId('artifact-toggle')).toBeInTheDocument();
    });

    it('displays artifact count', () => {
      (useArtifactState as unknown as ReturnType<typeof vi.fn>).mockReturnValue({
        artifacts: [
          { id: 'art-1', content: 'test1' },
          { id: 'art-2', content: 'test2' },
          { id: 'art-3', content: 'test3' },
        ],
      });

      render(<Header />);

      expect(screen.getByText('Artifacts (3)')).toBeInTheDocument();
    });

    it('toggles showArtifacts when clicked', () => {
      (useAppState as unknown as ReturnType<typeof vi.fn>).mockReturnValue({
        currentProject: { id: 'proj-1', name: 'Test Project' },
        showArtifacts: false,
        setShowArtifacts: mockSetShowArtifacts,
      });

      render(<Header />);

      const artifactToggle = screen.getByTestId('artifact-toggle');
      fireEvent.click(artifactToggle);

      expect(mockSetShowArtifacts).toHaveBeenCalledWith(true);
    });
  });

  describe('logout button', () => {
    it('renders logout button', () => {
      render(<Header />);

      expect(screen.getByTitle('Logout')).toBeInTheDocument();
    });

    it('calls logout and navigates to login when clicked', () => {
      render(<Header />);

      const logoutButton = screen.getByTitle('Logout');
      fireEvent.click(logoutButton);

      expect(mockLogout).toHaveBeenCalledTimes(1);
      expect(mockNavigate).toHaveBeenCalledWith('/login');
    });
  });
});
