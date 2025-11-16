// src/App.tsx
import { useEffect, useState } from 'react';
import { BrowserRouter, Routes, Route, Navigate } from 'react-router-dom';
import { Login } from './pages/Login';
import { Home } from './Home';
import { PrivateRoute } from './components/PrivateRoute';
import { useAuthStore } from './stores/useAuthStore';
import './App.css';

function App() {
  const [isVerifying, setIsVerifying] = useState(true);
  const { verifyToken, isAuthenticated } = useAuthStore();

  // Verify token on app load
  useEffect(() => {
    const verify = async () => {
      if (isAuthenticated) {
        await verifyToken();
      }
      setIsVerifying(false);
    };

    verify();
  }, []);

  // Show loading screen while verifying token
  if (isVerifying) {
    return (
      <div className="min-h-screen bg-gray-900 flex items-center justify-center">
        <div className="text-center">
          <div className="inline-block animate-spin rounded-full h-12 w-12 border-t-2 border-b-2 border-blue-500 mb-4"></div>
          <p className="text-gray-400">Loading...</p>
        </div>
      </div>
    );
  }

  return (
    <BrowserRouter>
      <Routes>
        <Route path="/login" element={<Login />} />
        <Route
          path="/"
          element={
            <PrivateRoute>
              <Home />
            </PrivateRoute>
          }
        />
        <Route path="*" element={<Navigate to="/" replace />} />
      </Routes>
    </BrowserRouter>
  );
}

export default App;
