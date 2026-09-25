// Components available in every .mdx page without an import.
import MDXComponents from '@theme-original/MDXComponents'
import Tabs from '@theme/Tabs'
import TabItem from '@theme/TabItem'
import Cards from '@site/src/components/Cards'
import Endpoint from '@site/src/components/Endpoint'
import TrackerGrid from '@site/src/components/TrackerGrid'

export default {
  ...MDXComponents,
  Tabs,
  TabItem,
  Cards,
  Endpoint,
  TrackerGrid,
}
