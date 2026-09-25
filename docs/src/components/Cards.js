import Link from '@docusaurus/Link'
import styles from './Cards.module.css'

/** Grid of link cards: items = [{ icon, title, text, to }] */
export default function Cards({ items, columns = 3 }) {
  return (
    <div className={styles.grid} style={{ '--cols': columns }}>
      {items.map((item) => (
        <Link key={item.title} className={styles.card} to={item.to}>
          <span className={styles.icon}>{item.icon}</span>
          <strong className={styles.title}>{item.title}</strong>
          <span className={styles.text}>{item.text}</span>
        </Link>
      ))}
    </div>
  )
}
