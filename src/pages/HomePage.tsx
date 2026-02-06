// src/pages/HomePage.tsx

import { useState, useEffect, useMemo } from 'react'
import { useNavigate } from 'react-router-dom'
import { invoke } from '@tauri-apps/api/core'
import { ScrollArea } from '../components/ui/scroll-area'
import { Separator } from '../components/ui/separator'
import { Button } from '../components/ui/button'
import { ButtonGroup } from '../components/ui/button-group'
import { Input } from '../components/ui/input'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from '../components/ui/dialog'
import { MessageCircle, MoreVertical, Plus, Check, X } from 'lucide-react'
import '../styles/HomePage.css'

interface FriendWithProfile {
  friend_id: string
  username: string
  nickname: string
  created_at: string
  is_online?: boolean
  avatar_url?: string | null
  status?: string | null
}

interface FriendRequestWithProfile {
  id: string
  from_user_id: string
  to_user_id: string
  status: string
  created_at: string
  from_username?: string
  from_nickname?: string
  from_avatar_url?: string | null
  from_status?: string | null
  to_username?: string
  to_nickname?: string
  to_avatar_url?: string | null
  to_status?: string | null
}

interface ConversationWithDetails {
  conversation_id: string
  conversation_type: string
  name: string | null
  other_user_id: string | null
  other_user_nickname: string | null
  other_user_avatar_url?: string | null
  other_user_status?: string | null
  last_message: string | null
  last_message_time: number | null
  has_unread: boolean
}

interface ProfileInfo {
  user_id: string
  nickname: string
  avatar_url: string | null
  status: string | null
}

interface FriendsResult {
  success: boolean
  error?: string
}

type FriendsTab = 'online' | 'all' | 'pending'
type Status = 'online' | 'idle' | 'dnd' | 'offline'

const STATUS_COLORS: Record<Status, string> = {
  online: '#22c55e',
  idle: '#eab308',
  dnd: '#ef4444',
  offline: '#6b7280',
}

// Avatar component with status indicator
interface AvatarProps {
  src?: string | null
  fallback: string
  size?: 'sm' | 'md' | 'lg'
  status?: string | null
  showStatus?: boolean
  className?: string
}

function Avatar({ src, fallback, size = 'md', status, showStatus = false, className = '' }: AvatarProps) {
  const sizeClasses = {
    sm: 'avatar-sm',
    md: 'avatar-md',
    lg: 'avatar-lg'
  }

  const statusColor = STATUS_COLORS[(status as Status) || 'offline']

  return (
    <div className={`avatar ${sizeClasses[size]} ${className}`}>
      {src ? (
        <img src={src} alt={fallback} className="avatar-image" />
      ) : (
        <span className="avatar-fallback">{fallback.charAt(0).toUpperCase()}</span>
      )}
      {showStatus && (
        <div 
          className="status-indicator" 
          style={{ backgroundColor: statusColor }}
        />
      )}
    </div>
  )
}

export default function HomePage() {
  const navigate = useNavigate()
  const [username, setUsername] = useState<string>('')
  const [friends, setFriends] = useState<FriendWithProfile[]>([])
  const [recentChats, setRecentChats] = useState<ConversationWithDetails[]>([])
  const [incomingRequests, setIncomingRequests] = useState<FriendRequestWithProfile[]>([])
  const [outgoingRequests, setOutgoingRequests] = useState<FriendRequestWithProfile[]>([])
  const [searchQuery, setSearchQuery] = useState('')
  const [friendsTab, setFriendsTab] = useState<FriendsTab>('all')
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [actionLoading, setActionLoading] = useState(false)

  // Add Friend Dialog state
  const [searchUsername, setSearchUsername] = useState('')
  const [addFriendLoading, setAddFriendLoading] = useState(false)
  const [addFriendError, setAddFriendError] = useState<string | null>(null)
  const [addFriendSuccess, setAddFriendSuccess] = useState<string | null>(null)

  useEffect(() => {
    loadData()
  }, [])

  const loadData = async () => {
    try {
      // Load profile
      const profile = await invoke<{ username: string } | null>('get_profile')
      if (profile) {
        setUsername(profile.username)
      }

      // Load friends
      const friendsData = await invoke<FriendWithProfile[]>('get_friends')

      // Load friend requests
      const [incoming, outgoing] = await Promise.all([
        invoke<FriendRequestWithProfile[]>('get_incoming_friend_requests'),
        invoke<FriendRequestWithProfile[]>('get_outgoing_friend_requests'),
      ])

      // Load recent conversations
      let conversations: ConversationWithDetails[] = []
      try {
        conversations = await invoke<ConversationWithDetails[]>('get_conversations')
      } catch (convErr) {
        console.log('No conversations yet or failed to load:', convErr)
      }

      // Collect all unique user IDs that need profile fetching
      const userIds = new Set<string>()
      
      friendsData.forEach(f => userIds.add(f.friend_id))
      incoming.forEach(r => userIds.add(r.from_user_id))
      outgoing.forEach(r => userIds.add(r.to_user_id))
      conversations.forEach(c => {
        if (c.other_user_id) userIds.add(c.other_user_id)
      })

      // Fetch all profiles at once
      let profilesMap = new Map<string, ProfileInfo>()
      if (userIds.size > 0) {
        try {
          const profiles = await invoke<ProfileInfo[]>('get_profiles_by_ids', {
            userIds: Array.from(userIds)
          })
          profiles.forEach(p => profilesMap.set(p.user_id, p))
        } catch (err) {
          console.error('Failed to fetch profiles:', err)
        }
      }

      // Merge profile data into friends
      const friendsWithProfiles = friendsData.map(f => ({
        ...f,
        avatar_url: profilesMap.get(f.friend_id)?.avatar_url || null,
        status: profilesMap.get(f.friend_id)?.status || null,
      }))

      // Merge profile data into incoming requests
      const incomingWithProfiles = incoming.map(r => ({
        ...r,
        from_avatar_url: profilesMap.get(r.from_user_id)?.avatar_url || null,
        from_status: profilesMap.get(r.from_user_id)?.status || null,
      }))

      // Merge profile data into outgoing requests
      const outgoingWithProfiles = outgoing.map(r => ({
        ...r,
        to_avatar_url: profilesMap.get(r.to_user_id)?.avatar_url || null,
        to_status: profilesMap.get(r.to_user_id)?.status || null,
      }))

      // Merge profile data into conversations
      const conversationsWithProfiles = conversations.map(c => ({
        ...c,
        other_user_avatar_url: c.other_user_id 
          ? profilesMap.get(c.other_user_id)?.avatar_url || null 
          : null,
        other_user_status: c.other_user_id
          ? profilesMap.get(c.other_user_id)?.status || null
          : null,
      }))

      setFriends(friendsWithProfiles)
      setIncomingRequests(incomingWithProfiles)
      setOutgoingRequests(outgoingWithProfiles)
      setRecentChats(conversationsWithProfiles)
    } catch (err) {
      console.error('Failed to load data:', err)
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setLoading(false)
    }
  }

  // Filter friends based on search query and tab
  const filteredFriends = useMemo(() => {
    let filtered = friends

    // Filter by tab - now using actual status
    if (friendsTab === 'online') {
      filtered = filtered.filter((friend) => 
        friend.status === 'online' || friend.status === 'idle' || friend.status === 'dnd'
      )
    }

    // Filter by search query
    if (searchQuery.trim()) {
      const query = searchQuery.toLowerCase()
      filtered = filtered.filter(
        (friend) =>
          friend.nickname.toLowerCase().includes(query) ||
          friend.username.toLowerCase().includes(query)
      )
    }

    return filtered
  }, [friends, searchQuery, friendsTab])

  // Count online friends (online, idle, or dnd - not offline/invisible)
  const onlineFriendsCount = useMemo(() => {
    return friends.filter((friend) => 
      friend.status === 'online' || friend.status === 'idle' || friend.status === 'dnd'
    ).length
  }, [friends])

  // Total pending requests
  const pendingCount = incomingRequests.length + outgoingRequests.length

  const handleCreateChat = () => {
    console.log('Create chat clicked')
  }

  const handleSendFriendRequest = async () => {
    if (!searchUsername.trim()) {
      setAddFriendError('Please enter a username')
      return
    }

    setAddFriendLoading(true)
    setAddFriendError(null)
    setAddFriendSuccess(null)

    try {
      const result = await invoke<FriendsResult>('send_friend_request', {
        toUsername: searchUsername.trim(),
      })

      if (result.success) {
        setAddFriendSuccess(`Friend request sent to ${searchUsername}!`)
        setSearchUsername('')
        loadData()
        setTimeout(() => {
          setAddFriendSuccess(null)
        }, 1500)
      } else {
        setAddFriendError(result.error || 'Failed to send friend request')
      }
    } catch (err) {
      console.error('Failed to send friend request:', err)
      setAddFriendError(err instanceof Error ? err.message : String(err))
    } finally {
      setAddFriendLoading(false)
    }
  }

  const handleAcceptRequest = async (requestId: string) => {
    setActionLoading(true)
    try {
      const result = await invoke<FriendsResult>('accept_friend_request', { requestId })
      if (result.success) {
        loadData()
      } else {
        setError(result.error || 'Failed to accept request')
      }
    } catch (err) {
      console.error('Failed to accept friend request:', err)
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setActionLoading(false)
    }
  }

  const handleDeclineRequest = async (requestId: string) => {
    setActionLoading(true)
    try {
      const result = await invoke<FriendsResult>('decline_friend_request', { requestId })
      if (result.success) {
        loadData()
      } else {
        setError(result.error || 'Failed to decline request')
      }
    } catch (err) {
      console.error('Failed to decline friend request:', err)
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setActionLoading(false)
    }
  }

  const handleCancelRequest = async (requestId: string) => {
    setActionLoading(true)
    try {
      const result = await invoke<FriendsResult>('cancel_friend_request', { requestId })
      if (result.success) {
        loadData()
      } else {
        setError(result.error || 'Failed to cancel request')
      }
    } catch (err) {
      console.error('Failed to cancel friend request:', err)
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setActionLoading(false)
    }
  }

  const handleOpenChat = (chat: ConversationWithDetails) => {
    if (chat.conversation_type === 'direct' && chat.other_user_id) {
      navigate(`/chat/${chat.other_user_id}`)
    }
  }

  const getChatDisplayName = (chat: ConversationWithDetails): string => {
    if (chat.conversation_type === 'direct') {
      return chat.other_user_nickname || 'Unknown'
    }
    return chat.name || 'Group Chat'
  }

  if (loading) {
    return (
      <div className="home-page">
        <div className="home-loading">Loading...</div>
      </div>
    )
  }

  return (
    <div className="home-page">
      <div className="home-header">
        <h1>Welcome{username ? `, ${username}` : ''}!</h1>
      </div>

      <div className="home-layout">
        {/* Recent Chats Panel */}
        <div className="recent-chats-panel">
          <div className="panel-header">
            <h2 className="panel-title">Recent Chats</h2>
            <button
              className="panel-action-button"
              onClick={handleCreateChat}
              title="Create Chat"
            >
              <Plus size={16} />
            </button>
          </div>
          <ScrollArea className="recent-chats-list-container">
            <div className="recent-chats-list">
              {recentChats.length > 0 ? (
                recentChats.map((chat) => (
                  <button
                    key={chat.conversation_id}
                    className={`recent-chat-item ${chat.has_unread ? 'unread' : ''}`}
                    onClick={() => handleOpenChat(chat)}
                  >
                    <Avatar
                      src={chat.other_user_avatar_url}
                      fallback={getChatDisplayName(chat)}
                      size="md"
                      status={chat.other_user_status}
                      showStatus
                      className="recent-chat-avatar"
                    />
                    <div className="recent-chat-info">
                      <span className="recent-chat-name">{getChatDisplayName(chat)}</span>
                      <span className="recent-chat-message">
                        {chat.last_message || 'No messages yet'}
                      </span>
                    </div>
                    {chat.has_unread && <div className="unread-indicator" />}
                  </button>
                ))
              ) : (
                <p className="panel-empty">No recent chats.</p>
              )}
            </div>
          </ScrollArea>
        </div>

        {/* Friends List */}
        <div className="home-content">
          <div className="panel-header">
            <h2 className="panel-title">Friends</h2>
            <ButtonGroup>
              <Button
                variant={friendsTab === 'online' ? 'default' : 'outline'}
                size="sm"
                onClick={() => setFriendsTab('online')}
              >
                Online ({onlineFriendsCount})
              </Button>
              <Button
                variant={friendsTab === 'all' ? 'default' : 'outline'}
                size="sm"
                onClick={() => setFriendsTab('all')}
              >
                All ({friends.length})
              </Button>
              <Button
                variant={friendsTab === 'pending' ? 'default' : 'outline'}
                size="sm"
                onClick={() => setFriendsTab('pending')}
              >
                Pending ({pendingCount})
              </Button>
            </ButtonGroup>
            <Dialog>
              <DialogTrigger asChild>
                <Button variant="outline" size="sm" className="border-green-600 text-green-600 hover:bg-green-600 hover:text-white">
                  Add Friend
                </Button>
              </DialogTrigger>
              <DialogContent className="bg-card text-card-foreground">
                <DialogHeader>
                  <DialogTitle>Add Friend</DialogTitle>
                  <DialogDescription>
                    Enter the username of the person you want to add as a friend.
                  </DialogDescription>
                </DialogHeader>
                
                <div className="add-friend-dialog-content">
                  {addFriendError && (
                    <p className="add-friend-dialog-error">{addFriendError}</p>
                  )}
                  {addFriendSuccess && (
                    <p className="add-friend-dialog-success">{addFriendSuccess}</p>
                  )}
                  <Input
                    type="text"
                    placeholder="Username"
                    value={searchUsername}
                    onChange={(e) => setSearchUsername(e.target.value)}
                    onKeyDown={(e) => e.key === 'Enter' && handleSendFriendRequest()}
                    disabled={addFriendLoading}
                  />
                </div>

                <DialogFooter>
                  <Button
                    onClick={handleSendFriendRequest}
                    disabled={addFriendLoading || !searchUsername.trim()}
                  >
                    {addFriendLoading ? 'Sending...' : 'Send Request'}
                  </Button>
                </DialogFooter>
              </DialogContent>
            </Dialog>
          </div>

          {/* Search - only show for online/all tabs */}
          {friendsTab !== 'pending' && (
            <div className="home-search">
              <input
                type="text"
                className="home-search-input"
                placeholder="Search friends..."
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
              />
            </div>
          )}

          {error && <p className="home-error">{error}</p>}

          {/* Pending Requests Tab */}
          {friendsTab === 'pending' ? (
            <ScrollArea className="friends-list-container">
              <div className="friends-list">
                {/* Incoming Requests */}
                {incomingRequests.length > 0 && (
                  <>
                    <div className="requests-section-header">
                      Incoming ({incomingRequests.length})
                    </div>
                    {incomingRequests.map((request, index) => (
                      <div key={request.id}>
                        <div className="friend-row">
                          <div className="friend-row-left">
                            <Avatar
                              src={request.from_avatar_url}
                              fallback={request.from_nickname || 'U'}
                              size="sm"
                              status={request.from_status}
                              showStatus
                              className="friend-avatar"
                            />
                            <div className="friend-info">
                              <span className="friend-name">{request.from_nickname || 'Unknown'}</span>
                              <span className="friend-username">@{request.from_username || 'unknown'}</span>
                            </div>
                          </div>
                          <div className="friend-row-actions">
                            <button
                              className="friend-action-icon accept"
                              onClick={() => handleAcceptRequest(request.id)}
                              disabled={actionLoading}
                              title="Accept"
                            >
                              <Check size={18} />
                            </button>
                            <button
                              className="friend-action-icon decline"
                              onClick={() => handleDeclineRequest(request.id)}
                              disabled={actionLoading}
                              title="Decline"
                            >
                              <X size={18} />
                            </button>
                          </div>
                        </div>
                        {index < incomingRequests.length - 1 && <Separator />}
                      </div>
                    ))}
                  </>
                )}

                {/* Separator between sections */}
                {incomingRequests.length > 0 && outgoingRequests.length > 0 && (
                  <div className="requests-divider" />
                )}

                {/* Outgoing Requests */}
                {outgoingRequests.length > 0 && (
                  <>
                    <div className="requests-section-header">
                      Outgoing ({outgoingRequests.length})
                    </div>
                    {outgoingRequests.map((request, index) => (
                      <div key={request.id}>
                        <div className="friend-row">
                          <div className="friend-row-left">
                            <Avatar
                              src={request.to_avatar_url}
                              fallback={request.to_nickname || 'U'}
                              size="sm"
                              status={request.to_status}
                              showStatus
                              className="friend-avatar"
                            />
                            <div className="friend-info">
                              <span className="friend-name">{request.to_nickname || 'Unknown'}</span>
                              <span className="friend-username">@{request.to_username || 'unknown'}</span>
                            </div>
                          </div>
                          <div className="friend-row-actions">
                            <button
                              className="friend-action-icon decline"
                              onClick={() => handleCancelRequest(request.id)}
                              disabled={actionLoading}
                              title="Cancel Request"
                            >
                              <X size={18} />
                            </button>
                          </div>
                        </div>
                        {index < outgoingRequests.length - 1 && <Separator />}
                      </div>
                    ))}
                  </>
                )}

                {/* Empty state */}
                {incomingRequests.length === 0 && outgoingRequests.length === 0 && (
                  <p className="home-empty">No pending friend requests.</p>
                )}
              </div>
            </ScrollArea>
          ) : (
            /* Friends List (Online/All tabs) */
            friends.length > 0 ? (
              <ScrollArea className="friends-list-container">
                <div className="friends-list">
                  {filteredFriends.length > 0 ? (
                    filteredFriends.map((friend, index) => (
                      <div key={friend.friend_id}>
                        <div className="friend-row">
                          <div className="friend-row-left">
                            <Avatar
                              src={friend.avatar_url}
                              fallback={friend.nickname}
                              size="sm"
                              status={friend.status}
                              showStatus
                              className="friend-avatar"
                            />
                            <div className="friend-info">
                              <span className="friend-name">{friend.nickname}</span>
                              <span className="friend-username">@{friend.username}</span>
                            </div>
                          </div>
                          <div className="friend-row-actions">
                            <button
                              className="friend-action-icon"
                              onClick={() => navigate(`/chat/${friend.friend_id}`)}
                              title="Message"
                            >
                              <MessageCircle size={18} />
                            </button>
                            <button
                              className="friend-action-icon"
                              onClick={() => {/* Future menu */}}
                              title="More options"
                            >
                              <MoreVertical size={18} />
                            </button>
                          </div>
                        </div>
                        {index < filteredFriends.length - 1 && <Separator />}
                      </div>
                    ))
                  ) : (
                    <p className="home-empty">
                      {friendsTab === 'online' 
                        ? 'No friends online.' 
                        : 'No friends match your search.'}
                    </p>
                  )}
                </div>
              </ScrollArea>
            ) : (
              <p className="home-empty">No friends yet. Add some friends to start chatting!</p>
            )
          )}
        </div>
      </div>
    </div>
  )
}