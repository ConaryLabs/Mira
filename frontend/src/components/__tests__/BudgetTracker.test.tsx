// src/components/__tests__/BudgetTracker.test.tsx
// BudgetTracker Component Tests

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { BudgetTracker } from '../BudgetTracker';
import { useBudgetStatus, useCodeIntelligenceStore, BudgetStatus } from '../../stores/useCodeIntelligenceStore';
import { useWebSocketStore } from '../../stores/useWebSocketStore';
import { useAuthStore } from '../../stores/useAuthStore';

// Mock the stores
vi.mock('../../stores/useCodeIntelligenceStore', () => ({
  useBudgetStatus: vi.fn(),
  useCodeIntelligenceStore: vi.fn(),
}));

vi.mock('../../stores/useWebSocketStore', () => ({
  useWebSocketStore: vi.fn(),
}));

vi.mock('../../stores/useAuthStore', () => ({
  useAuthStore: vi.fn(),
}));

const createMockBudget = (overrides: Partial<BudgetStatus> = {}): BudgetStatus => ({
  dailyUsagePercent: 25,
  monthlyUsagePercent: 10,
  dailySpentUsd: 1.25,
  dailyLimitUsd: 5.0,
  monthlySpentUsd: 15.0,
  monthlyLimitUsd: 150.0,
  dailyRemaining: 3.75,
  monthlyRemaining: 135.0,
  isCritical: false,
  isLow: false,
  lastUpdated: Date.now(),
  ...overrides,
});

describe('BudgetTracker', () => {
  let mockSend: ReturnType<typeof vi.fn>;
  let mockSetBudgetLoading: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    vi.clearAllMocks();

    mockSend = vi.fn().mockResolvedValue(undefined);
    mockSetBudgetLoading = vi.fn();

    // Default useWebSocketStore mock
    (useWebSocketStore as unknown as ReturnType<typeof vi.fn>).mockImplementation((selector) => {
      if (typeof selector === 'function') {
        return selector({ send: mockSend });
      }
      return { send: mockSend };
    });

    // Default useCodeIntelligenceStore mock
    (useCodeIntelligenceStore as unknown as ReturnType<typeof vi.fn>).mockImplementation((selector) => {
      if (typeof selector === 'function') {
        return selector({ setBudgetLoading: mockSetBudgetLoading });
      }
      return { setBudgetLoading: mockSetBudgetLoading };
    });

    // Default useAuthStore mock
    (useAuthStore as unknown as ReturnType<typeof vi.fn>).mockImplementation((selector) => {
      if (typeof selector === 'function') {
        return selector({ user: { id: 'user-123', username: 'testuser' } });
      }
      return { user: { id: 'user-123', username: 'testuser' } };
    });
  });

  describe('loading state', () => {
    it('shows loading state when isLoading is true', () => {
      (useBudgetStatus as ReturnType<typeof vi.fn>).mockReturnValue({
        budget: null,
        isLoading: true,
        error: null,
      });

      render(<BudgetTracker />);

      expect(screen.getByText('Loading budget data...')).toBeInTheDocument();
    });

    it('shows loading animation elements', () => {
      (useBudgetStatus as ReturnType<typeof vi.fn>).mockReturnValue({
        budget: null,
        isLoading: true,
        error: null,
      });

      const { container } = render(<BudgetTracker />);

      const pulsingElements = container.querySelectorAll('.animate-pulse');
      expect(pulsingElements.length).toBeGreaterThan(0);
    });
  });

  describe('error state', () => {
    it('shows error message when there is an error', () => {
      (useBudgetStatus as ReturnType<typeof vi.fn>).mockReturnValue({
        budget: null,
        isLoading: false,
        error: 'Connection failed',
      });

      render(<BudgetTracker />);

      expect(screen.getByText(/Failed to load budget: Connection failed/)).toBeInTheDocument();
    });
  });

  describe('no data state', () => {
    it('shows "no data available" when budget is null', () => {
      (useBudgetStatus as ReturnType<typeof vi.fn>).mockReturnValue({
        budget: null,
        isLoading: false,
        error: null,
      });

      render(<BudgetTracker />);

      expect(screen.getByText('No budget data available')).toBeInTheDocument();
    });
  });

  describe('data display', () => {
    it('shows budget overview header', () => {
      (useBudgetStatus as ReturnType<typeof vi.fn>).mockReturnValue({
        budget: createMockBudget(),
        isLoading: false,
        error: null,
      });

      render(<BudgetTracker />);

      expect(screen.getByText('Budget Overview')).toBeInTheDocument();
    });

    it('shows daily spent amount', () => {
      (useBudgetStatus as ReturnType<typeof vi.fn>).mockReturnValue({
        budget: createMockBudget({ dailySpentUsd: 2.50 }),
        isLoading: false,
        error: null,
      });

      render(<BudgetTracker />);

      expect(screen.getByText('$2.50')).toBeInTheDocument();
    });

    it('shows daily limit', () => {
      (useBudgetStatus as ReturnType<typeof vi.fn>).mockReturnValue({
        budget: createMockBudget({ dailyLimitUsd: 10.0 }),
        isLoading: false,
        error: null,
      });

      render(<BudgetTracker />);

      expect(screen.getByText('of $10.00')).toBeInTheDocument();
    });

    it('shows daily remaining', () => {
      (useBudgetStatus as ReturnType<typeof vi.fn>).mockReturnValue({
        budget: createMockBudget({ dailyRemaining: 7.50 }),
        isLoading: false,
        error: null,
      });

      render(<BudgetTracker />);

      expect(screen.getByText('$7.50')).toBeInTheDocument();
    });

    it('shows monthly spent amount', () => {
      (useBudgetStatus as ReturnType<typeof vi.fn>).mockReturnValue({
        budget: createMockBudget({ monthlySpentUsd: 45.0 }),
        isLoading: false,
        error: null,
      });

      render(<BudgetTracker />);

      expect(screen.getByText('$45.00')).toBeInTheDocument();
    });

    it('shows monthly limit', () => {
      (useBudgetStatus as ReturnType<typeof vi.fn>).mockReturnValue({
        budget: createMockBudget({ monthlyLimitUsd: 200.0 }),
        isLoading: false,
        error: null,
      });

      render(<BudgetTracker />);

      expect(screen.getByText('of $200.00')).toBeInTheDocument();
    });

    it('shows monthly remaining', () => {
      (useBudgetStatus as ReturnType<typeof vi.fn>).mockReturnValue({
        budget: createMockBudget({ monthlyRemaining: 155.0 }),
        isLoading: false,
        error: null,
      });

      render(<BudgetTracker />);

      expect(screen.getByText('$155.00')).toBeInTheDocument();
    });
  });

  describe('budget warnings', () => {
    it('shows critical warning when budget is critical', () => {
      (useBudgetStatus as ReturnType<typeof vi.fn>).mockReturnValue({
        budget: createMockBudget({ isCritical: true }),
        isLoading: false,
        error: null,
      });

      render(<BudgetTracker />);

      expect(screen.getByText(/Budget critical!/)).toBeInTheDocument();
      expect(screen.getByText(/Context gathering reduced to save costs/)).toBeInTheDocument();
    });

    it('shows low warning when budget is low but not critical', () => {
      (useBudgetStatus as ReturnType<typeof vi.fn>).mockReturnValue({
        budget: createMockBudget({ isLow: true, isCritical: false }),
        isLoading: false,
        error: null,
      });

      render(<BudgetTracker />);

      expect(screen.getByText(/Budget running low/)).toBeInTheDocument();
    });

    it('does not show warning when budget is healthy', () => {
      (useBudgetStatus as ReturnType<typeof vi.fn>).mockReturnValue({
        budget: createMockBudget({ isLow: false, isCritical: false }),
        isLoading: false,
        error: null,
      });

      render(<BudgetTracker />);

      expect(screen.queryByText(/Budget critical/)).not.toBeInTheDocument();
      expect(screen.queryByText(/Budget running low/)).not.toBeInTheDocument();
    });
  });

  describe('context level indicator', () => {
    it('shows "Full" when usage is low', () => {
      (useBudgetStatus as ReturnType<typeof vi.fn>).mockReturnValue({
        budget: createMockBudget({ dailyUsagePercent: 30, monthlyUsagePercent: 20 }),
        isLoading: false,
        error: null,
      });

      render(<BudgetTracker />);

      expect(screen.getByText('Full')).toBeInTheDocument();
    });

    it('shows "Standard" when usage is moderate', () => {
      (useBudgetStatus as ReturnType<typeof vi.fn>).mockReturnValue({
        budget: createMockBudget({ dailyUsagePercent: 50, monthlyUsagePercent: 50 }),
        isLoading: false,
        error: null,
      });

      render(<BudgetTracker />);

      expect(screen.getByText('Standard')).toBeInTheDocument();
    });

    it('shows "Minimal" when usage is high', () => {
      (useBudgetStatus as ReturnType<typeof vi.fn>).mockReturnValue({
        budget: createMockBudget({ dailyUsagePercent: 85, monthlyUsagePercent: 85 }),
        isLoading: false,
        error: null,
      });

      render(<BudgetTracker />);

      expect(screen.getByText('Minimal')).toBeInTheDocument();
    });
  });

  describe('refresh functionality', () => {
    it('renders refresh button', () => {
      (useBudgetStatus as ReturnType<typeof vi.fn>).mockReturnValue({
        budget: createMockBudget(),
        isLoading: false,
        error: null,
      });

      render(<BudgetTracker />);

      expect(screen.getByTitle('Refresh budget status')).toBeInTheDocument();
    });

    it('calls send to refresh budget when refresh button is clicked', async () => {
      (useBudgetStatus as ReturnType<typeof vi.fn>).mockReturnValue({
        budget: createMockBudget(),
        isLoading: false,
        error: null,
      });

      render(<BudgetTracker />);

      const refreshButton = screen.getByTitle('Refresh budget status');
      fireEvent.click(refreshButton);

      expect(mockSetBudgetLoading).toHaveBeenCalledWith(true);
      expect(mockSend).toHaveBeenCalledWith({
        type: 'code_intelligence_command',
        method: 'code.budget_status',
        params: { user_id: 'user-123' },
      });
    });

    it('does not request budget when user is not logged in', async () => {
      (useAuthStore as unknown as ReturnType<typeof vi.fn>).mockImplementation((selector) => {
        if (typeof selector === 'function') {
          return selector({ user: null });
        }
        return { user: null };
      });

      (useBudgetStatus as ReturnType<typeof vi.fn>).mockReturnValue({
        budget: createMockBudget(),
        isLoading: false,
        error: null,
      });

      render(<BudgetTracker />);

      const refreshButton = screen.getByTitle('Refresh budget status');
      fireEvent.click(refreshButton);

      expect(mockSend).not.toHaveBeenCalled();
    });
  });

  describe('last updated timestamp', () => {
    it('shows last updated time when available', () => {
      const mockTime = new Date('2025-01-15T14:30:00').getTime();
      (useBudgetStatus as ReturnType<typeof vi.fn>).mockReturnValue({
        budget: createMockBudget({ lastUpdated: mockTime }),
        isLoading: false,
        error: null,
      });

      render(<BudgetTracker />);

      expect(screen.getByText(/Updated/)).toBeInTheDocument();
    });

    it('does not show last updated when timestamp is 0', () => {
      (useBudgetStatus as ReturnType<typeof vi.fn>).mockReturnValue({
        budget: createMockBudget({ lastUpdated: 0 }),
        isLoading: false,
        error: null,
      });

      render(<BudgetTracker />);

      expect(screen.queryByText(/Updated/)).not.toBeInTheDocument();
    });
  });

  describe('progress bars', () => {
    it('renders daily budget progress bar', () => {
      (useBudgetStatus as ReturnType<typeof vi.fn>).mockReturnValue({
        budget: createMockBudget(),
        isLoading: false,
        error: null,
      });

      render(<BudgetTracker />);

      expect(screen.getByText('Daily Budget')).toBeInTheDocument();
    });

    it('renders monthly budget progress bar', () => {
      (useBudgetStatus as ReturnType<typeof vi.fn>).mockReturnValue({
        budget: createMockBudget(),
        isLoading: false,
        error: null,
      });

      render(<BudgetTracker />);

      expect(screen.getByText('Monthly Budget')).toBeInTheDocument();
    });
  });
});
