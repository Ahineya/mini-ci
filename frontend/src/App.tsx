import { Link, Navigate, Route, Routes, useLocation } from "react-router-dom";
import { ProjectDetail } from "./pages/ProjectDetail";
import { ProjectList } from "./pages/ProjectList";

function NavIcon() {
  return (
    <svg width="18" height="18" viewBox="0 0 18 18" fill="none" className="text-accent">
      <rect x="1" y="1" width="16" height="16" rx="4" stroke="currentColor" strokeWidth="1.5" />
      <path d="M5.5 9.5L7.5 11.5L12.5 6.5" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  );
}

export function App() {
  const location = useLocation();
  const isProjectPage = location.pathname.startsWith("/p/");

  return (
    <div className="flex h-full flex-col bg-surface-0">
      {/* Top bar */}
      <header className="flex h-12 shrink-0 items-center border-b border-border bg-surface-1/80 px-4 backdrop-blur-sm">
        <Link to="/" className="flex items-center gap-2 text-sm font-semibold text-text-primary">
          <NavIcon />
          <span>mini-ci</span>
        </Link>
        <div className="mx-3 h-4 w-px bg-border" />
        <nav className="flex items-center gap-1 text-sm">
          <Link
            to="/"
            className={`rounded-md px-2.5 py-1 transition ${
              !isProjectPage
                ? "bg-surface-3 text-text-primary"
                : "text-text-secondary hover:text-text-primary"
            }`}
          >
            Projects
          </Link>
        </nav>
      </header>

      {/* Content */}
      <main className="min-h-0 flex-1">
        <Routes>
          <Route path="/" element={<ProjectList />} />
          <Route path="/p/:id" element={<ProjectDetail />} />
          <Route path="*" element={<Navigate to="/" replace />} />
        </Routes>
      </main>
    </div>
  );
}
