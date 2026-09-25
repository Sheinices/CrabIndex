import { Route, Routes } from 'react-router'
import { Layout } from './components/Layout.jsx'
import { NotFoundPage } from './pages/NotFoundPage.jsx'
import { SearchPage } from './pages/SearchPage.jsx'
import { StatsPage } from './pages/StatsPage.jsx'

export function App() {
  return (
    <Routes>
      <Route element={<Layout />}>
        <Route index element={<SearchPage />} />
        <Route path="stats" element={<StatsPage />} />
        <Route path="*" element={<NotFoundPage />} />
      </Route>
    </Routes>
  )
}
