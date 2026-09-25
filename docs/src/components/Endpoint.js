import styles from './Endpoint.module.css'

/** HTTP method + path badge: <Endpoint method="GET" path="/api/v1.0/torrents" /> */
export default function Endpoint({ method = 'GET', path, children }) {
  return (
    <div className={styles.endpoint}>
      <span className={styles.method} data-method={method.toUpperCase()}>
        {method.toUpperCase()}
      </span>
      <code className={styles.path}>{path}</code>
      {children && <span className={styles.desc}>{children}</span>}
    </div>
  )
}
