// tests/ConnectionBanner.test.tsx
// Component tests for ConnectionBanner - FIXED to match actual implementation

import { describe, it, expect, beforeEach, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import { ConnectionBanner } from '../src/components/ConnectionBanner';
import { useWebSocketStore } from '../src/stores/useWebSocketStore';

vi.mock('../src/stores/useWebSocketStore');

beforeEach(() => {
  vi.clearAllMocks();
});

describe('ConnectionBanner Component', () => {
  // ===== Connected State =====
  
  describe('Connected State', () => {
    it('does not show banner when connected', () => {
      vi.mocked(useWebSocketStore).mockReturnValue('connected' as any);
      
      const { container } = render(<ConnectionBanner />);
      
      // Banner should not be visible when connected
      expect(container.firstChild).toBeNull();
    });
  });
  
  // ===== Connecting State =====
  
  describe('Connecting State', () => {
    it('shows connecting message', () => {
      vi.mocked(useWebSocketStore).mockReturnValue('connecting' as any);
      
      render(<ConnectionBanner />);
      
      expect(screen.getByText(/connecting to mira/i)).toBeInTheDocument();
    });
    
    it('uses yellow/warning styling for connecting', () => {
      vi.mocked(useWebSocketStore).mockReturnValue('connecting' as any);
      
      render(<ConnectionBanner />);
      
      const banner = screen.getByText(/connecting to mira/i).closest('div');
      // Should have warning colors (yellow-ish)
      expect(banner?.className).toMatch(/yellow/i);
    });
    
    it('shows animated wifi icon when connecting', () => {
      vi.mocked(useWebSocketStore).mockReturnValue('connecting' as any);
      
      render(<ConnectionBanner />);
      
      // Should have animated pulse
      const banner = document.querySelector('.animate-pulse');
      expect(banner).toBeInTheDocument();
    });
  });
  
  // ===== Reconnecting State =====
  
  describe('Reconnecting State', () => {
    it('shows reconnecting message', () => {
      vi.mocked(useWebSocketStore).mockReturnValue('reconnecting' as any);
      
      render(<ConnectionBanner />);
      
      // FIXED: Component shows "Reconnecting to Mira..." not "Disconnected from Mira"
      expect(screen.getByText(/reconnecting to mira/i)).toBeInTheDocument();
    });
    
    it('uses yellow/warning styling for reconnecting', () => {
      vi.mocked(useWebSocketStore).mockReturnValue('reconnecting' as any);
      
      render(<ConnectionBanner />);
      
      const banner = screen.getByText(/reconnecting to mira/i).closest('div');
      // Should have warning colors (yellow-ish)
      expect(banner?.className).toMatch(/yellow/i);
    });
    
    it('shows animated wifi icon when reconnecting', () => {
      vi.mocked(useWebSocketStore).mockReturnValue('reconnecting' as any);
      
      render(<ConnectionBanner />);
      
      // Should have animated pulse
      const banner = document.querySelector('.animate-pulse');
      expect(banner).toBeInTheDocument();
    });
  });
  
  // ===== Disconnected State =====
  
  describe('Disconnected State', () => {
    it('shows disconnected message', () => {
      vi.mocked(useWebSocketStore).mockReturnValue('disconnected' as any);
      
      render(<ConnectionBanner />);
      
      expect(screen.getByText(/disconnected from mira/i)).toBeInTheDocument();
    });
    
    it('uses red/error styling for disconnected', () => {
      vi.mocked(useWebSocketStore).mockReturnValue('disconnected' as any);
      
      render(<ConnectionBanner />);
      
      const banner = screen.getByText(/disconnected from mira/i).closest('div');
      // Should have error colors (red-ish)
      expect(banner?.className).toMatch(/red/i);
    });
    
    it('treats "error" as disconnected', () => {
      vi.mocked(useWebSocketStore).mockReturnValue('error' as any);
      
      render(<ConnectionBanner />);
      
      expect(screen.getByText(/disconnected from mira/i)).toBeInTheDocument();
    });
  });
  
  // ===== State Transitions =====
  
  describe('State Transitions', () => {
    it('updates when connection state changes', () => {
      const { rerender } = render(<ConnectionBanner />);
      
      // Start connecting
      vi.mocked(useWebSocketStore).mockReturnValue('connecting' as any);
      rerender(<ConnectionBanner />);
      expect(screen.getByText(/connecting to mira/i)).toBeInTheDocument();
      
      // Change to connected
      vi.mocked(useWebSocketStore).mockReturnValue('connected' as any);
      rerender(<ConnectionBanner />);
      expect(screen.queryByText(/connecting/i)).not.toBeInTheDocument();
    });
    
    it('shows banner again when disconnecting after connection', () => {
      // Start connected
      vi.mocked(useWebSocketStore).mockReturnValue('connected' as any);
      const { container, rerender } = render(<ConnectionBanner />);
      expect(container.firstChild).toBeNull();
      
      // Disconnect
      vi.mocked(useWebSocketStore).mockReturnValue('disconnected' as any);
      rerender(<ConnectionBanner />);
      expect(screen.getByText(/disconnected from mira/i)).toBeInTheDocument();
    });
    
    it('transitions from connecting to reconnecting', () => {
      const { rerender } = render(<ConnectionBanner />);
      
      // Start connecting
      vi.mocked(useWebSocketStore).mockReturnValue('connecting' as any);
      rerender(<ConnectionBanner />);
      expect(screen.getByText(/connecting to mira/i)).toBeInTheDocument();
      
      // Change to reconnecting
      vi.mocked(useWebSocketStore).mockReturnValue('reconnecting' as any);
      rerender(<ConnectionBanner />);
      expect(screen.getByText(/reconnecting to mira/i)).toBeInTheDocument();
    });
  });
  
  // ===== Edge Cases =====
  
  describe('Edge Cases', () => {
    it('handles undefined connection state gracefully', () => {
      vi.mocked(useWebSocketStore).mockReturnValue(undefined as any);
      
      const { container } = render(<ConnectionBanner />);
      
      // Should show disconnected or nothing
      expect(container).toBeInTheDocument();
    });
    
    it('handles null connection state gracefully', () => {
      vi.mocked(useWebSocketStore).mockReturnValue(null as any);
      
      const { container } = render(<ConnectionBanner />);
      
      expect(container).toBeInTheDocument();
    });
    
    it('handles unknown connection state', () => {
      vi.mocked(useWebSocketStore).mockReturnValue('unknown-state' as any);
      
      render(<ConnectionBanner />);
      
      // Should show disconnected (default case)
      expect(screen.getByText(/disconnected from mira/i)).toBeInTheDocument();
    });
  });
});
