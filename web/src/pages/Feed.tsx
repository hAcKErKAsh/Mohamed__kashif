import { useState } from 'react'
import { useInfiniteQuery, useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Lightbulb, MessageCircle, PartyPopper, Hand, Trash2, Send } from 'lucide-react'
import { api, qs } from '../lib/api'
import { useSession } from '../lib/session'
import { unitLabel, unitsWithPerm, useUnits } from '../lib/units'
import { Avatar, Empty, PageHead, Spinner, ago, errorText, useToast } from '../components/ui'

interface Post {
  id: string; body: string; status: string; created_at: string; author: string; author_id: string; author_role: string | null
  unit: string; unit_id: string; reactions: Record<string, number>; my_reaction: string | null; comment_count: number; can_moderate: boolean
}
interface Comment { id: string; body: string; author: string; author_id: string; created_at: string; can_remove: boolean }

const REACTIONS = [
  ['appreciate', 'Appreciate', Hand],
  ['celebrate', 'Celebrate', PartyPopper],
  ['insightful', 'Insightful', Lightbulb],
] as const

export default function Feed() {
  const { me, can } = useSession()
  const qc = useQueryClient()
  const toast = useToast()
  const { data: units } = useUnits()
  const postable = unitsWithPerm(units, me.grants, 'social.post')
  const viewable = unitsWithPerm(units, me.grants, 'social.view')
  const [filter, setFilter] = useState('')
  const [body, setBody] = useState('')
  const [audience, setAudience] = useState('')
  const direct = me.grants.some((g) => g.perms.includes('social.post_direct'))

  const feed = useInfiniteQuery({
    queryKey: ['feed', filter],
    queryFn: ({ pageParam }) => api.get<Post[]>(`/feed${qs({ before: pageParam, unit: filter })}`),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (last) => (last.length === 30 ? last[last.length - 1].created_at : undefined),
  })
  const posts = feed.data?.pages.flat()

  const create = useMutation({
    mutationFn: () => api.post<{ status: string }>('/feed', { body, org_unit_id: audience || postable[0]?.id }),
    onSuccess: (r) => {
      setBody('')
      toast(r.status === 'pending' ? 'Sent for a quick check by a teacher. It will appear once approved.' : 'Posted.')
      qc.invalidateQueries({ queryKey: ['feed'] })
    },
    onError: (e) => toast(errorText(e), 'bad'),
  })

  return (
    <main className="content">
      <PageHead eyebrow="Community" title="What's happening">
        Posts reach the group you choose and everyone above it. Kind, useful, and school-appropriate, please.
      </PageHead>
      <div className="split">
        <div className="stack" style={{ gap: 20 }}>
          {can('social.post') && postable.length > 0 && (
            <form className="panel" onSubmit={(e) => { e.preventDefault(); if (body.trim()) create.mutate() }}>
              <div className="panel-pad stack" style={{ gap: 12 }}>
                <div className="row" style={{ alignItems: 'flex-start', gap: 12 }}>
                  <Avatar name={me.user.full_name} />
                  <textarea className="textarea" aria-label="Write a post" placeholder="Share something with your class or school…" value={body} onChange={(e) => setBody(e.target.value)} maxLength={5000} style={{ minHeight: 76 }} />
                </div>
                <div className="row wrap" style={{ justifyContent: 'space-between', paddingLeft: 44 }}>
                  <label className="row" style={{ gap: 8, fontSize: 13 }}>
                    <span className="faint">Audience</span>
                    <select className="select sm" style={{ width: 'auto' }} value={audience || postable[0]?.id} onChange={(e) => setAudience(e.target.value)}>
                      {postable.map((u) => <option key={u.id} value={u.id}>{unitLabel(u)}</option>)}
                    </select>
                  </label>
                  <div className="row" style={{ gap: 10 }}>
                    {!direct && <span className="faint" style={{ fontSize: 12.5 }}>Reviewed before it appears</span>}
                    <button className="btn primary sm" disabled={!body.trim() || create.isPending}><Send /> Post</button>
                  </div>
                </div>
              </div>
            </form>
          )}
          {!posts && <Spinner />}
          {posts?.length === 0 && <div className="panel"><Empty title="It's quiet here">Be the first to share something.</Empty></div>}
          {posts?.map((p) => <PostCard key={p.id} post={p} />)}
          {feed.hasNextPage && (
            <button className="btn" onClick={() => feed.fetchNextPage()} disabled={feed.isFetchingNextPage}>Show older posts</button>
          )}
        </div>
        <aside className="panel">
          <div className="panel-head"><h3>Groups</h3></div>
          <div className="panel-pad" style={{ display: 'flex', flexDirection: 'column', gap: 1 }}>
            <button className={`tree-row ${filter === '' ? 'on' : ''}`} style={{ border: 0, width: '100%', textAlign: 'left', background: filter === '' ? undefined : 'transparent' }} onClick={() => setFilter('')}>Everything I can see</button>
            {viewable.map((u) => (
              <button key={u.id} className={`tree-row ${filter === u.id ? 'on' : ''}`} style={{ border: 0, width: '100%', textAlign: 'left', paddingLeft: 8 + (u.depth - 1) * 14, background: filter === u.id ? undefined : 'transparent' }} onClick={() => setFilter(u.id)}>
                {u.name}
              </button>
            ))}
          </div>
        </aside>
      </div>
    </main>
  )
}

function PostCard({ post }: { post: Post }) {
  const { me } = useSession()
  const qc = useQueryClient()
  const toast = useToast()
  const [showComments, setShowComments] = useState(false)
  const [text, setText] = useState('')
  const comments = useQuery({ queryKey: ['comments', post.id], queryFn: () => api.get<Comment[]>(`/feed/${post.id}/comments`), enabled: showComments })
  const react = useMutation({
    mutationFn: (kind: string | null) => api.post(`/feed/${post.id}/react`, { kind }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['feed'] }),
    onError: (e) => toast(errorText(e), 'bad'),
  })
  const comment = useMutation({
    mutationFn: () => api.post(`/feed/${post.id}/comments`, { body: text }),
    onSuccess: () => {
      setText('')
      qc.invalidateQueries({ queryKey: ['comments', post.id] })
      qc.invalidateQueries({ queryKey: ['feed'] })
    },
    onError: (e) => toast(errorText(e), 'bad'),
  })
  const remove = useMutation({
    mutationFn: () => api.post(`/feed/${post.id}/remove`),
    onSuccess: () => { toast('Post removed.'); qc.invalidateQueries({ queryKey: ['feed'] }) },
  })
  const removeComment = useMutation({
    mutationFn: (id: string) => api.post(`/feed/comments/${id}/remove`),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['comments', post.id] }),
  })
  const mine = post.author_id === me.user.id
  return (
    <article className="panel post" id={post.id}>
      <div className="post-head">
        <Avatar name={post.author} />
        <div style={{ minWidth: 0, flex: 1 }}>
          <div style={{ fontWeight: 500 }}>{post.author}</div>
          <div className="faint" style={{ fontSize: 12.5 }}>{post.author_role ? `${post.author_role} · ` : ''}to {post.unit} · {ago(post.created_at)}</div>
        </div>
        {post.status === 'pending' && <span className="tag warn"><span className="dot" />Awaiting review</span>}
        {(mine || post.can_moderate) && (
          <button className="btn ghost icon sm" title={mine ? 'Delete post' : 'Remove as moderator'} aria-label="Remove post" onClick={() => confirm('Remove this post?') && remove.mutate()}><Trash2 /></button>
        )}
      </div>
      <p className="post-body">{post.body}</p>
      {post.status === 'published' && (
        <div className="post-actions">
          {REACTIONS.map(([k, label, Icon]) => (
            <button key={k} className={`react-btn ${post.my_reaction === k ? 'on' : ''}`} onClick={() => react.mutate(post.my_reaction === k ? null : k)} aria-pressed={post.my_reaction === k}>
              <Icon /> {label} {post.reactions[k] ? <span className="num">{post.reactions[k]}</span> : null}
            </button>
          ))}
          <button className="react-btn" onClick={() => setShowComments((s) => !s)} aria-expanded={showComments}>
            <MessageCircle /> {post.comment_count ? `${post.comment_count} comment${post.comment_count === 1 ? '' : 's'}` : 'Comment'}
          </button>
        </div>
      )}
      {showComments && (
        <div className="comments">
          {comments.data?.map((c) => (
            <div key={c.id} className="comment">
              <Avatar name={c.author} size="sm" />
              <div className="bubble">
                <div className="row between"><strong style={{ fontWeight: 500, fontSize: 13 }}>{c.author}</strong><span className="faint" style={{ fontSize: 11.5 }}>{ago(c.created_at)}</span></div>
                <div style={{ whiteSpace: 'pre-wrap', overflowWrap: 'anywhere' }}>{c.body}</div>
              </div>
              {c.can_remove && <button className="btn ghost icon sm" aria-label="Remove comment" onClick={() => removeComment.mutate(c.id)}><Trash2 /></button>}
            </div>
          ))}
          <form className="row" onSubmit={(e) => { e.preventDefault(); if (text.trim()) comment.mutate() }}>
            <input className="input sm" placeholder="Write a comment…" value={text} onChange={(e) => setText(e.target.value)} maxLength={2000} aria-label="Write a comment" />
            <button className="btn sm" disabled={!text.trim() || comment.isPending}>Reply</button>
          </form>
        </div>
      )}
    </article>
  )
}
